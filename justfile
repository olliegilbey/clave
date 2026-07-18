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

# Everything (host + plugin).
build-all: build build-bar

# --workspace is LOAD-BEARING: default-members excludes the wasm-only
# clave-bar crate, so bare `cargo test` silently skips all 33 model.rs
# tests — the divergence-critical ones (testing-strategy finding #1).
test:
    cargo test --workspace

# Copy both artifacts where the generated layout/config expect them:
# the wasm into ~/.local/share/clave/, the binary onto PATH.
install: build build-bar-release
    mkdir -p ~/.local/share/clave
    cp target/wasm32-wasip1/release/clave-bar.wasm ~/.local/share/clave/
    cargo install --path crates/clave --locked

clippy:
    cargo clippy --workspace --all-targets -- -D warnings
