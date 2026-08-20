#!/usr/bin/env bash
# frecency-walk.sh — one nav-ring walk for the frecency E2E drive (PR #218).
# Anchors the executor at $1 (a tab id), then presses {"row":N} for each row
# 1..$2 and prints the focused tab after each landing. Landings are the
# drive's machine-readable read-back of rows() order (dump-screen returns
# nothing for plugin panes — FOOTGUNS).
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1
ANCHOR="${1:?usage: frecency-walk.sh <anchor-tab-id> <row-count>}"
ROWS="${2:?usage: frecency-walk.sh <anchor-tab-id> <row-count>}"

focused() {
  scripts/ct.sh dump-layout 2>/dev/null | grep -o 'tab name="[^"]*" focus=true' \
    | sed 's/ focus=true//'
}

echo "start: $(focused)"
scripts/ct.sh pipe --name clave-visited -- "$ANCHOR" </dev/null || { echo "ANCHOR REFUSED"; exit 1; }
for n in $(seq 1 "$ROWS"); do
  scripts/ct.sh pipe --name clave-nav -- "{\"row\":$n}" </dev/null || { echo "NAV $n REFUSED"; exit 1; }
  sleep 2
  echo "row$n: $(focused)"
done
