#!/usr/bin/env bash
# frecency-quiescence.sh — SOP step 6 for the PR #218 drive: 60s idle, then
# assert the store seq and the sandbox evlog both held still (the anti-storm
# check). Prints both readings either way, per TESTING.md — a moving reading
# is a finding to attribute, not an automatic red.
set -euo pipefail
STATE=/Users/olliegilbey/.local/state/clave-dev-frecency-735d/state
seq_now() { jq -r '.seq' "$STATE/agents.json"; }
ev_now()  { wc -l < "$STATE/clave.log" | tr -d ' '; }
bar_now() { grep -c 'clave-bar' "${TMPDIR%/}/zellij-$(id -u)/zellij-log/zellij.log"; }
S0=$(seq_now); E0=$(ev_now); B0=$(bar_now)
echo "t0: seq=$S0 evlog=$E0 barlog=$B0"
sleep 60
S1=$(seq_now); E1=$(ev_now); B1=$(bar_now)
echo "t60: seq=$S1 evlog=$E1 barlog=$B1"
if [[ "$S0" == "$S1" && "$E0" == "$E1" ]]; then
  echo "QUIESCENT: seq and evlog frozen over 60s (barlog delta $((B1-B0)) is user-global, forensic only)"
else
  echo "MOVED: seq $S0->$S1 evlog $E0->$E1 — attribute before calling the drive green"
fi
