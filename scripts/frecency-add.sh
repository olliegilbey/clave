#!/usr/bin/env bash
# frecency-add.sh — run `clave add` inside a sandbox repo for the PR #218
# newborn-inheritance permutation. Env is stripped/redirected so the new tab
# and the snapshot push land in the SANDBOX session (FOOTGUNS: the store env
# does not sandbox the session).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="${1:?usage: frecency-add.sh <sandbox-repo-dir>}"
# Ask THIS checkout's binary which sandbox it owns (ct.sh doctrine — a
# hardcoded path targets someone else's sandbox from another worktree).
# CLAVE_BIN pins everything at once for a caller that needs to.
CLAVE_BIN="${CLAVE_BIN:-$ROOT/target/release/clave}"
STATE="$("$CLAVE_BIN" dev instance --field state)"
DATA="$("$CLAVE_BIN" dev instance --field data)"
SHIM="$("$CLAVE_BIN" dev instance --field shim)"
SESSION="$("$CLAVE_BIN" dev instance --field session)"
cd "$REPO"
env -u ZELLIJ -u ZELLIJ_PANE_ID \
  ZELLIJ_SESSION_NAME="$SESSION" \
  CLAVE_STATE_DIR="$STATE" CLAVE_DATA_DIR="$DATA" \
  PATH="$SHIM:$PATH" \
  "$CLAVE_BIN" add
