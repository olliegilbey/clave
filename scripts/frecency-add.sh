#!/usr/bin/env bash
# frecency-add.sh — run `clave add` inside a sandbox repo for the PR #218
# newborn-inheritance permutation. Env is stripped/redirected so the new tab
# and the snapshot push land in the SANDBOX session (FOOTGUNS: the store env
# does not sandbox the session).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="${1:?usage: frecency-add.sh <sandbox-repo-dir>}"
STATE=/Users/olliegilbey/.local/state/clave-dev-frecency-735d/state
DATA=/Users/olliegilbey/.local/state/clave-dev-frecency-735d/data
SHIM=/Users/olliegilbey/.local/state/clave-dev-frecency-735d/shim
cd "$REPO"
env -u ZELLIJ -u ZELLIJ_PANE_ID \
  ZELLIJ_SESSION_NAME=clave-test-frecency-735d \
  CLAVE_STATE_DIR="$STATE" CLAVE_DATA_DIR="$DATA" \
  PATH="$SHIM:$PATH" \
  "$ROOT/target/release/clave" add
