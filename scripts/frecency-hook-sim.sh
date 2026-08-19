#!/usr/bin/env bash
# frecency-hook-sim.sh — simulate a UserPromptSubmit commitment for one seeded
# agent in THIS worktree's sandbox store (PR #218 drive; the seeded rows hold
# no promptable claude, so the hook path is driven directly, and the report
# labels it as a simulation). Zellij env is stripped/redirected so the hook's
# snapshot push lands in the SANDBOX session, never the maintainer's fleet
# (FOOTGUNS: CLAVE_STATE_DIR sandboxes the store, not the session).
set -euo pipefail
UUID="${1:?usage: frecency-hook-sim.sh <agent-uuid> [event]}"
EVENT="${2:-UserPromptSubmit}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE=/Users/olliegilbey/.local/state/clave-dev-frecency-735d/state
DATA=/Users/olliegilbey/.local/state/clave-dev-frecency-735d/data
echo '{}' | env -u ZELLIJ -u ZELLIJ_PANE_ID \
  ZELLIJ_SESSION_NAME=clave-test-frecency-735d \
  CLAVE_AGENT_UUID="$UUID" \
  CLAVE_STATE_DIR="$STATE" CLAVE_DATA_DIR="$DATA" \
  "$ROOT/target/release/clave" hook "$EVENT"
jq -c --arg u "$UUID" '{seq, ord: .agents[$u].commit_ord, buckets: .agents[$u].buckets}' "$STATE/agents.json"
