# clave build orchestration. `just` with no target lists the recipes.
default:
    @just --list

# One-time: add the Zellij plugin's WASM target.
setup-toolchain:
    rustup target add wasm32-wasip1

# Host build — skips the WASM-only clave-bar via default-members.
build:
    cargo build

# Build the Zellij plugin to WASM (debug).
build-bar:
    cargo build -p clave-bar --target wasm32-wasip1

# Build the plugin release artifact.
build-bar-release:
    cargo build -p clave-bar --release --target wasm32-wasip1

# Local release-parity build: the CLI with the bar wasm EMBEDDED (spec
# §Distribution) — what cargo-dist produces in CI, buildable on any clone.
#
# The wasm build is inlined rather than depending on `build-bar-release` for
# the reason #109 hit `release`: a `just` dependency is its own recipe
# invocation with its own shell, so a tag set in THIS body never reaches it and
# the embedded bar reports `build=dev` (#167). Both lines carry the same tag
# expression release CI uses — the exact tag when HEAD is tagged, else the
# short SHA — so a parity build is distinguishable from a dev-install one.
dist-build:
    CLAVE_BUILD_TAG=$(git describe --tags --exact-match HEAD 2>/dev/null || git rev-parse --short HEAD) cargo build -p clave-bar --release --target wasm32-wasip1
    CLAVE_BAR_WASM=$(pwd)/target/wasm32-wasip1/release/clave-bar.wasm CLAVE_BUILD_TAG=$(git describe --tags --exact-match HEAD 2>/dev/null || git rev-parse --short HEAD) cargo build --release -p clave

# Everything (host + plugin).
build-all: build build-bar

# --workspace is LOAD-BEARING: default-members excludes the wasm-only
# clave-bar crate, so bare `cargo test` silently skips all 33 model.rs
# tests — the divergence-critical ones (testing-strategy finding #1).
test:
    cargo test --workspace

# Every gate CI enforces, in CI's own order. `fmt --check` FIRST because that
# is where the lint job starts: #66 went red on three hand-edited files while
# every *documented* gate was locally green (the docs listed three commands and
# CI ran four).
# Run every CI gate (fmt, test, wasm, clippy) — use this before you push.
gates:
    cargo fmt --all --check
    cargo test --workspace
    cargo build -p clave-bar --target wasm32-wasip1
    cargo clippy --workspace --all-targets -- -D warnings

# §2: the working-tree build for the sandbox + contributor shells. Builds the
# bar wasm (build-tagged with the short SHA so the zellij log says which wasm
# produced a trace) straight into the SANDBOX data dir, and installs the dev
# CLI as ~/.cargo/bin/clave-dev. `just install` is RETIRED: working-tree
# installs straight under the daily environment were the foot-gun this split
# removes.
#
# The name is the fix (#43b, prod incident 2026-07-22). This used to run
# `cargo install --path`, which writes ~/.cargo/bin/`clave` — the SAME name the
# daily surface answers to. A stale working-tree build therefore won the cold
# start, wrote a launch.kdl baking ITS version's paths beside a config.kdl at
# the released version, and zellij loaded two plugin locations: a second
# sidebar in every tab, dead navigation. `clave-dev` collides with nothing;
# #43a gives the daily surface its own launcher at
# ~/.local/share/clave/bin/clave.
#
# Still not a free action while daily-driving: it rebuilds the SANDBOX wasm
# (~/.local/state/clave-dev/data/clave-bar.wasm) in place, so it must not run
# against a live clave-test session — `just sandbox` refuses for you and is the
# reviewed path for sandbox validation.
#
# AND that path is hardcoded to the MAIN checkout's sandbox. The sandbox is
# per-worktree now (crates/clave/src/sandbox.rs), but this recipe predates it
# and its steps are one-shell-each, so it cannot ask `clave dev instance`
# before it has built the CLI. Run from a worktree, it therefore writes the
# main checkout's data dir — the one `clave-test` uses — which is a second
# reason `just sandbox` is the reviewed path (#31). If a pre-#43b ~/.cargo/bin/clave is
# still on this machine it shadows the #43a launcher — check what it is
# (`command -v clave; clave --version`) before removing it.
#
# `--locked` is kept from the retired `cargo install` line, and extended to the
# wasm build (CodeRabbit CLI, 2026-07-25): both halves of a dev install must
# come from the committed lockfile, or the bar and the CLI can be built against
# different dependency resolutions. Staged + `mv`
# rather than `cp` over the destination, for the reason install_launcher
# documents — cp truncates the existing inode, which may be a running process
# image (ETXTBSY on Linux, a live text segment on macOS); mv within one
# filesystem is a rename, so a running clave-dev keeps its own inode. The
# staging name carries $$ (the shell's pid): two concurrent dev-installs — two
# agents in two worktrees is the normal case here — would otherwise share one
# temp path and could publish a half-written executable (CodeRabbit, #70).
# Build the working-tree wasm (into the sandbox) and install `clave-dev` (§2).
dev-install:
    mkdir -p ~/.local/state/clave-dev/data ~/.cargo/bin
    CLAVE_BUILD_TAG=$(git rev-parse --short HEAD 2>/dev/null || echo dev) cargo build -p clave-bar --release --locked --target wasm32-wasip1
    cp target/wasm32-wasip1/release/clave-bar.wasm ~/.local/state/clave-dev/data/
    CLAVE_BUILD_TAG=$(git rev-parse --short HEAD 2>/dev/null || echo dev) cargo build -p clave --release --locked
    cp target/release/clave ~/.cargo/bin/.clave-dev.$$.tmp
    chmod 755 ~/.cargo/bin/.clave-dev.$$.tmp
    mv -f ~/.cargo/bin/.clave-dev.$$.tmp ~/.cargo/bin/clave-dev
    @echo "installed ~/.cargo/bin/clave-dev — the daily clave launcher is untouched (#43b)"
    @echo "NOTE: the wasm went to the MAIN checkout's sandbox (~/.local/state/clave-dev/data),"
    @echo "      not this worktree's. Use \`just sandbox\` to stage a worktree (#31)."

# Cut a release (§2). `clave release` is the GATE: it refuses unless the tree
# is clean AND HEAD carries the exact vX.Y.Z tag matching Cargo.toml — so a
# dirty or untagged HEAD installs nothing. On a good cut it installs the
# versioned wasm + CLI copy under ~/.local/share/clave/ and regenerates stable
# config/layout/hooks so every generated reference points at the VERSIONED
# paths. A running session, pinned to the files baked at its launch, is
# untouched until its next cold start (running-session immunity). The build
# tag is the exact tag when HEAD is tagged, else `untagged` (the gate then
# refuses, cleanly).
#
# The wasm build is inlined here rather than depending on `build-bar-release`
# (#109): a `just` dependency runs as its own recipe invocation with its own
# shell, so `CLAVE_BUILD_TAG` set in THIS recipe's body never reached it — the
# wasm always built untagged and every released bar logged `build=dev`, even
# though the log line exists precisely to catch two builds of the same version
# (FOOTGUNS.md). Both artifacts now carry the same tag, matching the pattern
# `dev-install` already uses.
# CLAVE_BAR_WASM is what makes the installed binary a RELEASE binary, not
# merely a binary a release installed. `run_setup` (setup.rs) tells the two
# environments apart by `release::embedded_wasm()`, and without the embed a
# locally cut launcher answers "dev build" about itself: its own `clave setup`
# takes the dev branch, meets the guard that protects a release install from a
# dev binary, and refuses. `clave rows` regenerates through that same function,
# so on 2026-09-14 it wrote the store, failed, and left config.kdl describing
# the old geometry — two plugin identities, a second sidebar, dead navigation.
# Every cut from v0.1.0 to v0.5.0 shipped without the embed. The embed also
# makes a local cut the same KIND of artifact cargo-dist ships, which is what
# `dist-build` claims parity with one recipe above.
# Gate on a clean, vX.Y.Z-tagged HEAD, then install versioned artifacts (§2).
release:
    CLAVE_BUILD_TAG=$(git describe --tags --exact-match HEAD 2>/dev/null || echo untagged) cargo build -p clave-bar --release --target wasm32-wasip1
    CLAVE_BAR_WASM=$(pwd)/target/wasm32-wasip1/release/clave-bar.wasm CLAVE_BUILD_TAG=$(git describe --tags --exact-match HEAD 2>/dev/null || echo untagged) cargo build --release -p clave
    ./target/release/clave release \
        --wasm-src target/wasm32-wasip1/release/clave-bar.wasm \
        --cli-src target/release/clave

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Mutation-test the lines this branch changed, relative to `main` (or any base
# you pass). DELIBERATELY NOT in `just gates`: gates run on every PR and must
# stay fast, and a full mutation run over model.rs alone is enormous — a gate
# nobody can afford to run is a gate nobody runs. Which change classes owe a
# run, and what to do with a survivor, are in docs/dev/TESTING.md.
#
# TWO filters, for two different reasons.
#
# --in-diff scopes generation to lines the diff touches, so the cost tracks the
# size of the change rather than the size of the crate. Config (including the
# load-bearing `test_workspace = true`) is .cargo/mutants.toml.
#
# --iterate then skips the mutants an earlier run already caught, because
# --in-diff alone re-tests the WHOLE branch diff every time and that diff only
# grows: #261 reached 136 mutants and 17 minutes, of which 123 were already
# known good. Measured on that branch, a re-run went 17m -> 52s (135 of 136
# excluded, and the one it ran was the open survivor).
#
# KNOW WHAT THE CACHE KEY IS. cargo-mutants names a mutant by file, line,
# column and description. So it re-tests a mutant when the mutant's IDENTITY
# moves, not when the line's MEANING changes. Measured 2026-09-15: editing
# `(ordinal, 0)` to `(ordinal, 7)` in clave_types::live_key produced no new
# mutant to test. Two limits follow, and only the first one bites:
#   - It cannot hide a BROKEN test. cargo-mutants runs the unmutated baseline
#     first and aborts when that is red (measured the same day).
#   - It CAN hide a WEAKENED one, because a test that asserts less still passes
#     the baseline, and the mutant it used to catch is skipped.
# So the cache is dropped whenever the config or the toolchain moves (below),
# and `just mutants-cold` drops it on demand — run that once before the PR.
#
# `--workspace` is the SAME footgun again and there is no config key for it:
# cargo-mutants GENERATES mutants only for the default packages, and
# default-members excludes clave-bar — so without it a change to model.rs or
# render.rs silently produces zero mutants and reports success.
# Mutation-test the lines changed vs `main` — a finding tool, not a gate.
mutants base="main" *args:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v cargo-mutants >/dev/null || {
        echo "cargo-mutants not installed: cargo install cargo-mutants --locked" >&2
        exit 127
    }
    # The cache is only true for the config and the toolchain that filled it. A
    # new exclude rule, or a new compiler, can change a verdict that the cache
    # would then hide. So key it on both, and start cold when the key moves.
    # Linux is a first-class target and ships `sha256sum`, not `shasum`;
    # macOS ships `shasum`. Neither is a documented prerequisite, and under
    # `set -euo pipefail` a missing one kills the recipe before it generates a
    # single mutant. Try both, then say which to install. (CodeRabbit, #261)
    if command -v sha256sum >/dev/null; then
        cfg="$(sha256sum .cargo/mutants.toml)"
    elif command -v shasum >/dev/null; then
        cfg="$(shasum -a 256 .cargo/mutants.toml)"
    else
        echo "a SHA-256 command is required: install sha256sum or shasum" >&2
        exit 127
    fi
    key="$(cargo mutants --version) $(rustc --version) $cfg"
    if [ "$(cat target/.mutants-cache-key 2>/dev/null || true)" != "$key" ]; then
        echo "mutants: config or toolchain moved — the cache is dropped"
        rm -rf mutants.out mutants.out.old
        mkdir -p target
        printf '%s\n' "$key" >target/.mutants-cache-key
    fi
    diff=$(mktemp)
    trap 'rm -f "$diff"' EXIT
    # --merge-base so a stale local `main` does not report every commit since
    # the fork point as changed.
    git diff --merge-base {{ base }} -- '*.rs' >"$diff"
    if [ ! -s "$diff" ]; then
        echo "no changed Rust lines vs {{ base }} — nothing to mutate"
        exit 0
    fi
    cargo mutants --workspace --in-diff "$diff" --iterate {{ args }}

# The same run with the cache dropped — the only run that re-tests a mutant an
# earlier run caught. Run it once before you open the PR, because that is where
# a test you deleted since would show up.
# Re-run every mutant vs `main`, ignoring the cache — do this before a PR.
mutants-cold base="main" *args:
    rm -rf mutants.out mutants.out.old target/.mutants-cache-key
    just mutants {{ base }} {{ args }}

# One module, whole. The deliberate deep run: use it when a file is new or has
# been rewritten, where --in-diff would mutate everything anyway.
# Mutation-test one file in full (e.g. crates/clave-bar/src/render.rs).
mutants-file file *args:
    cargo mutants --workspace --file {{ file }} {{ args }}

# Wire THIS WORKING TREE's sandbox without touching the daily surface.
# The safe alternative to `dev-install` for sandbox validation: nothing here
# writes ~/.cargo/bin/clave (the name a LIVE session's plugin shells out to —
# the 2026-07-22 outage), and it refuses to run against a live session of its
# own. The instance is per-worktree (`clave dev instance`), so a sibling
# agent's live sandbox no longer blocks this one and cannot be written by it;
# the run also reaps sandboxes whose worktree is gone, and self-checks that it
# left every other agent's root byte-identical.
# Prints the launch command; never launches (session lifecycle is the human's).
# Sandbox-validate this working tree WITHOUT installing to the daily surface.
sandbox scenario="c8-cold-start":
    ./scripts/sandbox-setup.sh {{scenario}}

# Stage, wait for the launch, drive. The WHOLE loop in one command, because
# the old one was three messages wide — stage, ask the human, be told, drive —
# and every extra round trip is a chance to drive something that is not the
# sandbox. This prints the launch command and blocks until the session
# appears; launching is still the human's and nothing here starts a session.
#
# The drive scrubs the inherited zellij identity before its first phase, so
# every child it spawns aims at the sandbox rather than at whatever fleet this
# terminal happens to sit inside — and `clave hook` refuses a push whose
# target does not own the store it wrote (hook.rs `aim_push`), which the
# drive's own P6b witness phase then counts. All three exist because the
# 2026-09-11 drive hung the maintainer's live session.
#
# The drive is NOT idempotent and cannot be: phases 2-5 bind dormant rows,
# mint a row and churn tabs, consuming the starting shape they assert
# against. So a second run needs a fresh stage — which is why this recipe
# stages every time, and why phase 1 now names a stale stage outright
# instead of failing on a count that is reading the previous run.
# Re-staging needs the session down, so the loop is: kill, `just qa`, launch.
#
# THE MAINTAINER'S COMMAND, and his alone. `clave dev launch` refuses when
# ZELLIJ is set, so an agent — always inside a session — cannot run this; it
# gets a refusal that tells it to hand the command over instead. Everything
# the launch needs is derived from the working tree: the session name, the
# state and data dirs, and the PATH shim that makes a bare `clave` resolve to
# THIS build instead of the stable install.
#
# Run it in a new terminal window OUTSIDE zellij.
#
# It runs the staged binary and does NOT build one. `just qa` builds and stages
# in the same breath, and the drive's preflight then vouches for that exact
# build by tag — a rebuild here could hand the session a different binary than
# the one the preflight read, and nothing downstream would say so. So a missing
# build is an error with the staging command in it, not a silent rebuild.
launch:
    @test -x ./target/release/clave || { echo 'no staged build: run "just qa" first (it builds and stages), or "just dist-build" for a bare one'; exit 1; }
    ./target/release/clave dev launch

# Stage + wait for the human's launch + drive phases 0-7, in one command.
#
# It asks for a SECOND launch part way through: phase 6c asks the maintainer to
# quit the sandbox and launch it again, because the live set can only decay
# across a session
# boundary and no other phase crosses one. The drive prints both commands and
# waits; `wait` is the budget for EACH ask.
#
# Half an hour, measured: ten minutes closed on both asks on 2026-09-16, and
# each closed window costs a re-stage. The drive is asking a person to walk to
# another window, so the budget is set for somebody who came back to it rather
# than somebody watching it.
qa scenario="qa-fleet" wait="1800":
    ./scripts/sandbox-setup.sh {{scenario}}
    QA_WAIT_SECS={{wait}} QA_RELAUNCH_WAIT={{wait}} ./scripts/qa-drive.sh {{scenario}}
