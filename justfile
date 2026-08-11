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
dist-build: build-bar-release
    CLAVE_BAR_WASM=$(pwd)/target/wasm32-wasip1/release/clave-bar.wasm cargo build --release -p clave

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
# Gate on a clean, vX.Y.Z-tagged HEAD, then install versioned artifacts (§2).
release:
    CLAVE_BUILD_TAG=$(git describe --tags --exact-match HEAD 2>/dev/null || echo untagged) cargo build -p clave-bar --release --target wasm32-wasip1
    CLAVE_BUILD_TAG=$(git describe --tags --exact-match HEAD 2>/dev/null || echo untagged) cargo build --release -p clave
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
# --in-diff scopes generation to lines the diff touches, so the cost tracks the
# size of the change rather than the size of the crate. Config (including the
# load-bearing `test_workspace = true`) is .cargo/mutants.toml.
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
    diff=$(mktemp)
    trap 'rm -f "$diff"' EXIT
    # --merge-base so a stale local `main` does not report every commit since
    # the fork point as changed.
    git diff --merge-base {{ base }} -- '*.rs' >"$diff"
    if [ ! -s "$diff" ]; then
        echo "no changed Rust lines vs {{ base }} — nothing to mutate"
        exit 0
    fi
    cargo mutants --workspace --in-diff "$diff" {{ args }}

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
