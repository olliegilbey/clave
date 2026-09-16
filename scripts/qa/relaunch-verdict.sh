#!/usr/bin/env bash
# Phase 6c's verdict, run on its own against a sandbox that is ALREADY in the
# pre-quit state a full drive left it in.
#
# The drive has no phase resume, so a run whose 6c timed out waiting for the
# maintainer would otherwise cost a complete re-drive — two more launches — to
# re-reach a state the sandbox is already holding. This runs the SAME verdict
# function (`relaunch_checks`, qa/lib.sh) over the same readings, so what it
# proves is what the phase proves. It is not a substitute for a drive: it
# assumes phases 0–6b already ran and left the store flat.
#
# Usage, after the maintainer has quit and relaunched the sandbox:
#   scripts/qa/relaunch-verdict.sh <before-set-file> [closed-uuid]
# where <before-set-file> holds the bound uuids read BEFORE the quit, one per
# line — captured while the first session was still up, because the quit is
# what destroys them.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=./lib.sh
source "$ROOT/scripts/qa/lib.sh"
CLAVE_BIN="${CLAVE_BIN:-$ROOT/target/release/clave}"

BEFORE_FILE="${1:?pass the file holding the pre-quit bound uuids}"
CLOSED="${2:-}"
BEFORE="$(cat "$BEFORE_FILE")"

AFTER="$(dev_status)"
if [[ -z "$AFTER" ]]; then
  echo "FAILED: dev status returned nothing — is the sandbox up?" >&2
  exit 1
fi

relaunch_checks "$BEFORE" "$AFTER" "$CLOSED"
