#!/usr/bin/env bash
# seek-storm-watch.sh — the kill-condition monitor for #137 (width-seek storm).
#
# The storm killed a fleet by exhausting the zellij server's file descriptors
# (EMFILE at zellij-utils ipc.rs:388, which panics the accept loop). That is
# measurable long before the panic, so this samples it beside the drive.
#
# READ-ONLY: `ps` and `lsof` only. It never issues a zellij command, so it is
# safe to run while the maintainer's own fleet is up — and it deliberately
# watches EVERY zellij server, because the two sessions died together and
# whether they share a server is still an open question.
#
# Usage:  scripts/seek-storm-watch.sh [interval_secs] [out.csv]
set -uo pipefail

interval="${1:-2}"
out="${2:-/tmp/clave-137-fdwatch.csv}"

# Match the server processes, not the clients. Zellij names them
# `zellij --server <socket-path>`; the socket path carries the session name.
servers() {
  ps -Ao pid=,command= | awk '/zellij --server/ && !/awk/ {
    pid=$1; sock=$NF; n=split(sock, parts, "/"); print pid "," parts[n]
  }'
}

pipes_in_flight() {
  ps -Ao command= | grep -c '[z]ellij pipe'
}

if [[ ! -s "$out" ]]; then
  echo "ts,server_pid,session,fds,unix_sockets,pipe_clients" >"$out"
fi

echo "watching every ${interval}s -> $out  (ctrl-c to stop)" >&2
while :; do
  ts="$(date +%H:%M:%S)"
  pipes="$(pipes_in_flight)"
  while IFS=, read -r pid session; do
    [[ -z "${pid:-}" ]] && continue
    # lsof on a process we do not own can fail; a blank reading must not be
    # written as a zero, which would read as "fds dropped to nothing".
    all="$(lsof -p "$pid" 2>/dev/null | tail -n +2 | wc -l | tr -d ' ')"
    [[ -z "$all" || "$all" == "0" ]] && all="NA"
    unix="$(lsof -p "$pid" 2>/dev/null | grep -c unix | tr -d ' ')"
    printf '%s,%s,%s,%s,%s,%s\n' "$ts" "$pid" "$session" "$all" "$unix" "$pipes" >>"$out"
    printf '%s  %-8s %-14s fds=%-5s unix=%-4s pipes=%s\n' \
      "$ts" "$pid" "$session" "$all" "$unix" "$pipes" >&2
  done < <(servers)
  sleep "$interval"
done
