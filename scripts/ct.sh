#!/usr/bin/env bash
# ct.sh — run a zellij action against the clave-test SANDBOX, or refuse.
#
# Why this exists (2026-08-07): `ZELLIJ_SESSION_NAME=clave-test zellij action …`
# reads like a boundary and is not one. An agent shell inside the maintainer's
# session also inherits `ZELLIJ` and `ZELLIJ_PANE_ID`; override the name alone
# and it works right up until `clave-test` stops existing — at which point the
# CLI FALLS BACK to the ambient session instead of erroring. A whole queued
# drive then lands on the daily fleet. It did: a sandbox-built debug bar became
# a real pane in the maintainer's live session, and ten collapse toggles went to
# his fleet.
#
# So this wrapper fails CLOSED. It proves the sandbox is live, clears the
# ambient session, and only then runs. Use it for every drive command:
#
#     scripts/ct.sh new-tab --name t1
#     scripts/ct.sh start-or-reload-plugin "file:$SANDBOX/clave-bar.wasm" -c clave_binary=clave
#
# It deliberately does NOT accept a session name. There is exactly one session
# an agent may drive, and making it an argument would make the mistake
# expressible again.
set -euo pipefail

SESSION="clave-test"
SOCKET="${TMPDIR:-/tmp}/zellij-$(id -u)/contract_version_1/${SESSION}"

if [[ $# -eq 0 ]]; then
  echo "usage: $0 <zellij-action> [args…]   (runs against ${SESSION} only)" >&2
  exit 2
fi

# 1. The socket must exist. A dead session leaves nothing here, which is
#    precisely the case that used to fall through to the maintainer's fleet.
if [[ ! -S "$SOCKET" ]]; then
  cat >&2 <<EOF
REFUSING: no ${SESSION} session socket at
  ${SOCKET}

The sandbox is not running, so this command would have been served by whatever
session your shell is attached to — the maintainer's fleet. Ask him to launch
the sandbox (staging is 'just sandbox'; launching is his), then retry.
EOF
  exit 1
fi

# 2. A stale socket file outlives its server. Require a live server process
#    holding this exact path, so a leftover socket cannot green the check.
if ! pgrep -f "zellij --server ${SOCKET}" >/dev/null 2>&1; then
  echo "REFUSING: socket exists but no 'zellij --server ${SOCKET}' process is alive." >&2
  echo "That is a stale socket from a dead sandbox — treat it as not running." >&2
  exit 1
fi

# 3. Clear the ambient session so there is nothing to fall back to even if a
#    future zellij changes how it resolves a target. Belt as well as braces:
#    the name is set explicitly AND the inherited context is gone.
unset ZELLIJ ZELLIJ_PANE_ID
export ZELLIJ_SESSION_NAME="$SESSION"

exec zellij --session "$SESSION" action "$@"
