#!/usr/bin/env bash
# Wire the clave-test SANDBOX to this working tree, without installing anything
# onto the daily surface. The safe alternative to `just dev-install` for
# sandbox validation.
#
# Why this exists (#43/#44, 2026-07-22): `just dev-install` runs `cargo install`,
# which writes ~/.cargo/bin/clave — the SAME binary name a LIVE session's plugin
# shells out to for open/bind/snapshot. Running it while the maintainer is daily
# driving silently repoints the running fleet at a working-tree build. That is
# what broke production. Nothing here writes ~/.cargo/bin or ~/.local/share/clave:
# artifacts go to the sandbox data dir, and the working-tree CLI is reached
# through a PATH shim scoped to the printed launch command.
#
# The shim is NOT a convenience — it is load-bearing. The sandbox data dir holds
# no versioned CLI copy, so `release::runtime_binary()` bakes bare `clave` into
# the sandbox's generated KDL (by design, §2 binary split). The bar therefore
# resolves `clave` through PATH at runtime. Without the shim that resolves to
# the STABLE ~/.cargo/bin/clave — very possibly built BEFORE the change under
# test — so the bar drives the sandbox with the wrong binary and the run fails
# for a reason unrelated to the change. On #44 that misfire is especially
# convincing: a pre-#44 `clave open` composes tab layouts with no `clave_binary`,
# whose empty configuration is a DIFFERENT plugin identity from the template's
# bar, so tabs sprout a second sidebar — the exact symptom #44 fixes.
set -euo pipefail

SCENARIO="${1:-c8-cold-start}"
ROOT="$(git rev-parse --show-toplevel)"
SANDBOX="$HOME/.local/state/clave-dev"
SHIM="$SANDBOX/shim"
CLI="$ROOT/target/release/clave"

cd "$ROOT"

# mtime (or "absent") for a path, on BSD and GNU stat alike — clave must
# eventually run over SSH onto Linux, so no macOS-only stat flags.
stamp() {
  if [ -e "$1" ]; then
    stat -f %m "$1" 2>/dev/null || stat -c %Y "$1" 2>/dev/null || echo unknown
  else
    echo absent
  fi
}

STABLE_CLI="$HOME/.cargo/bin/clave"
STABLE_DIR="$HOME/.local/share/clave"
before_cli="$(stamp "$STABLE_CLI")"
before_dir="$(stamp "$STABLE_DIR")"

echo "==> Building the working tree (release, build-tagged)"
TAG="$(git rev-parse --short HEAD 2>/dev/null || echo dev)"
CLAVE_BUILD_TAG="$TAG" cargo build -p clave-bar --release --target wasm32-wasip1
CLAVE_BUILD_TAG="$TAG" cargo build -p clave --release

echo "==> Placing the bar wasm in the sandbox data dir"
mkdir -p "$SANDBOX/data"
cp target/wasm32-wasip1/release/clave-bar.wasm "$SANDBOX/data/"

echo "==> Wiring the PATH shim ($SHIM/clave -> this build)"
mkdir -p "$SHIM"
ln -sf "$CLI" "$SHIM/clave"

# REFUSE to regenerate against a live session. Zellij watches the --config file
# of every running session and hot-swaps its keybinds in place (zellij-server
# src/lib.rs:2175 -> ConfigWrittenToDisk :2298 -> ScreenInstruction::Reconfigure
# screen.rs:717, ~1s poll), while the on-screen bar keeps the plugin identity it
# loaded with. Rewriting config.kdl under a live clave-test therefore re-keys its
# keybinds to an identity that bar does not have, and the next keypress STARTS A
# SECOND BAR. Kill first, regenerate second.
# `-n` (no formatting) is REQUIRED: a bare `list-sessions` wraps each name in
# ANSI colour codes, so the line starts with an escape sequence and `^clave-test`
# never matches — the guard would silently never fire. `setup.rs::session_exists`
# uses `-n` for the same reason.
if zellij list-sessions -n 2>/dev/null | awk '{print $1}' | grep -qx 'clave-test'; then
  cat <<'DEAD' >&2
FAILED: a clave-test session is live, and regenerating config.kdl under it
would re-key its keybinds while the running bar keeps its load-time identity
— the next keypress would spawn a second bar (#44). Kill it first, in a
non-zellij terminal:

  zellij kill-session clave-test && zellij delete-session --force clave-test

DEAD
  exit 1
fi

# Regenerate BOTH config.kdl and launch.kdl. `dev launch` composes launch.kdl
# fresh but does NOT regenerate config.kdl, so launching without re-seeding
# pairs a fresh launch.kdl with a stale config.kdl — and since #44 both bake
# `clave_binary`, a stale pair makes every keybind miss and spawn a second bar.
# That reproduces the bug under test and reads as the fix failing.
echo "==> Seeding scenario '$SCENARIO' (regenerates config.kdl + launch.kdl)"
PATH="$SHIM:$PATH" "$CLI" dev reset
PATH="$SHIM:$PATH" "$CLI" dev scenario "$SCENARIO"

ok=1
fail() { echo "    FAIL $*"; ok=0; }

# Prove the generated pair agrees BEFORE a human spends time in a terminal.
# This is the #44 invariant: config.kdl's MessagePlugin keybinds and the layout's
# plugin node must carry an IDENTICAL clave_binary, or zellij resolves them as
# different plugins and every keypress opens another bar.
#
# The pair checked here is config.kdl <-> layout.kdl, because those are the two
# write_generated() emits together from one `binary` value. launch.kdl is NOT
# checked: only a cold start writes it (setup.rs launch_session), so before the
# launch this script is preparing for, it is either absent or a LEFTOVER from a
# previous run. Asserting on it here failed closed on a perfectly healthy
# sandbox — a stale pre-#44 launch.kdl from days earlier reported
# "carries no clave_binary" and blocked the setup entirely. Same defect the
# release runbook had against the same file (see RELEASE-RUNBOOK Step 2).
#
# The stale copy is deleted rather than ignored: leaving a pre-#44 launch layout
# on disk is the hazard itself, and the next cold start rewrites it regardless.
rm -f "$SANDBOX/data/launch.kdl"

echo
echo "==> Self-check: the #44 identity pair"
cfg="$SANDBOX/data/config.kdl"
lay="$SANDBOX/data/layout.kdl"
for f in "$cfg" "$lay"; do
  if [ ! -f "$f" ]; then
    fail "missing $f"
  elif ! grep -q 'clave_binary' "$f"; then
    fail "$(basename "$f") carries no clave_binary — pre-#44 artifact"
  else
    echo "    ok   $(basename "$f") carries clave_binary"
  fi
done
if [ "$ok" -eq 1 ]; then
  cfgval="$(grep -o 'clave_binary "[^"]*"' "$cfg" | sort -u)"
  layval="$(grep -o 'clave_binary "[^"]*"' "$lay" | sort -u)"
  if [ "$cfgval" = "$layval" ]; then
    echo "    ok   both sides agree: $cfgval"
  else
    fail "identity pair DISAGREES — every keybind would spawn a second bar"
    echo "         config.kdl: $cfgval"
    echo "         layout.kdl: $layval"
  fi
fi

echo
echo "==> Stable surfaces untouched"
# if/else, not `A && B || C`: this is a SAFETY guard, and in the `||` form a
# failing echo would report a bogus breach (shellcheck SC2015).
if [ "$(stamp "$STABLE_CLI")" = "$before_cli" ]; then
  echo "    ok   $STABLE_CLI unchanged"
else
  fail "$STABLE_CLI CHANGED — this script must never write it"
fi
if [ "$(stamp "$STABLE_DIR")" = "$before_dir" ]; then
  echo "    ok   $STABLE_DIR unchanged"
else
  fail "$STABLE_DIR CHANGED — this script must never write it"
fi

[ "$ok" -eq 1 ] || { echo; echo "Setup FAILED — do not launch."; exit 1; }

# Session lifecycle is the human's: print, never run. A `zellij action` against
# a dead session blocks forever, and only the human can see the screen.
echo
cat <<EOF
==> Ready. Launch it YOURSELF, in a NEW terminal window OUTSIDE zellij:

    PATH="$SHIM:\$PATH" "$CLI" dev launch

    The PATH shim is required, not cosmetic — without it the sandbox bar
    shells out to the stable ~/.cargo/bin/clave (see the header comment).

    Your live 'clave' session is untouched: nothing here wrote ~/.cargo/bin.
EOF
