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
# lib.sh's header names what a caller must set before sourcing. This script
# is a caller like any other, and it skipped the list: `print_summary` reads
# all five and runs under `set -u`, so a red verdict died in the summary
# instead of reporting one (found in review).
SCENARIO="${SCENARIO:-relaunch-verdict}"
DRIVE_LOG="${DRIVE_LOG:-(not a drive — this tool prints to the terminal only)}"
ZLOG="${ZLOG:-${TMPDIR:-/tmp}/zellij-$(id -u)/zellij-log/zellij.log}"
LOGMARK="${LOGMARK:-0}"
BUILD_TAG="${BUILD_TAG:-$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo dev)}"
# shellcheck source=./lib.sh
source "$ROOT/scripts/qa/lib.sh"
CLAVE_BIN="${CLAVE_BIN:-$ROOT/target/release/clave}"
CT="${CT:-$ROOT/scripts/ct.sh}"

BEFORE_FILE="${1:?pass the file holding the pre-quit bound uuids}"
CLOSED="${2:-}"
BEFORE="$(cat "$BEFORE_FILE")"

# `check` reports against the OPEN phase and `fail_phase` ends the run through
# it. With no phase open the whole verdict fell through and exited 0, whatever
# it printed — so opening one is what makes this tool's exit code mean
# something (found in review, reproduced: a red check exited 0).
phase "relaunch-verdict"

AFTER="$(dev_status)"
if [[ -z "$AFTER" ]]; then
  echo "FAILED: dev status returned nothing — is the sandbox up?" >&2
  exit 1
fi

relaunch_checks "$BEFORE" "$AFTER" "$CLOSED"
print_summary
