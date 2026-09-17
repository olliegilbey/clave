#!/usr/bin/env bash
# Sample a zellij server's open-handle count while a session comes up.
#
# Written after a RESTORE of five tabs crashed the sandbox server with
# "Too many open files" at the macOS default soft limit of 256, while the same
# session RUNNING seven tabs sat steady at 68. The gap is the finding: the
# building costs something the running does not. The curve and the handle KIND
# at the peak are what identify it.
#
# Reads only. Starts nothing, kills nothing, and sends no command to any
# zellij session — `lsof` inspects a process, it does not talk to the server.
#
# The server can die within a second of the crash, so the sampling loop has NO
# sleep in it: `lsof` itself paces it at roughly ten samples a second. The wait
# for the server to appear is the only paced part, and it counts real seconds —
# the first version counted ITERATIONS as seconds and so gave up in half the
# time it claimed, which is why the first restore crash went unmeasured.
#
# Usage: fd-sampler.sh <session-name> [seconds-to-wait-for-the-server]
set -uo pipefail

# `lsof` is both the reading and the PACING of the sampling loop, which has no
# sleep on purpose. Missing, it fails through `2>/dev/null` into an empty
# snapshot: the loop then spins at full CPU for the life of the server and
# writes `n=0` every turn, so the log says "no handles" rather than "no tool".
# (CodeRabbit, #261)
command -v lsof >/dev/null || {
  echo "lsof is required: it is both the reading and the pacing of the sampling loop" >&2
  exit 127
}

SESSION="${1:?pass the sandbox session name}"
DURATION="${2:-900}"
OUT="${FD_SAMPLE_OUT:-/tmp/fd-sample-$SESSION.log}"

: >"$OUT"
echo "waiting up to ${DURATION}s for the server of $SESSION" >>"$OUT"

DEADLINE=$(( $(date +%s) + DURATION ))
PID=""
while [[ -z "$PID" ]]; do
  PID="$(pgrep -f -- "--server.*$SESSION" | head -1)"
  [[ -n "$PID" ]] && break
  (( $(date +%s) >= DEADLINE )) && break
  sleep 0.2
done

if [[ -z "$PID" ]]; then
  echo "no server appeared for $SESSION within ${DURATION}s" >>"$OUT"
  exit 1
fi
echo "server pid=$PID at $(date +%H:%M:%S)" >>"$OUT"

PEAK=0
PEAK_BREAKDOWN=""
# Keep the last few breakdowns too: the peak may be the moment of death, and
# what was climbing just BEFORE it is what names the cause.
while kill -0 "$PID" 2>/dev/null; do
  SNAP="$(lsof -n -P -p "$PID" 2>/dev/null)"
  # NR>1 drops lsof's header row, exactly as the breakdown below does. Counted
  # in, every sample read one handle high against the 256 ceiling this script
  # exists to watch (CodeRabbit).
  N="$(printf '%s\n' "$SNAP" | awk 'NR>1' | grep -c .)"
  BREAK="$(printf '%s\n' "$SNAP" | awk 'NR>1 {print $5}' | sort | uniq -c | sort -rn | tr '\n' ' ')"
  printf '%s n=%s %s\n' "$(date +%H:%M:%S)" "$N" "$BREAK" >>"$OUT"
  if [[ "$N" -gt "$PEAK" ]]; then
    PEAK="$N"
    PEAK_BREAKDOWN="$BREAK"
  fi
done

{
  echo "--- server gone at $(date +%H:%M:%S) ---"
  echo "peak handles: $PEAK"
  echo "breakdown at the peak: $PEAK_BREAKDOWN"
} >>"$OUT"
