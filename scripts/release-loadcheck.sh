#!/usr/bin/env bash
# release-loadcheck.sh — Part C step 4, read-only: lines appended to the
# zellij log after the maintainer's logmark, filtered to bar loads. Prints
# the mark, the loaded-version lines, and says "empty" out loud (runbook:
# empty is never silently a pass).
set -uo pipefail
TMP="${TMPDIR:-/tmp}"
ZLOG="${TMP%/}/zellij-$(id -u)/zellij-log/zellij.log"
MARKFILE="${TMP%/}/clave-release-logmark"
if [[ ! -f "$MARKFILE" ]]; then
  echo "NO LOGMARK at $MARKFILE — step 1 not run (or a different TMPDIR)"
  exit 0
fi
MARK=$(cat "$MARKFILE")
echo "logmark: $MARK   log lines now: $(wc -l < "$ZLOG" | tr -d ' ')"
LOADED=$(tail -n +$((MARK + 1)) "$ZLOG" | grep "clave-bar: loaded v" || true)
if [[ -z "$LOADED" ]]; then
  echo "loaded-lines: empty (no bar load after the mark — cold start not happened yet, or wrong mark)"
else
  echo "$LOADED"
fi
