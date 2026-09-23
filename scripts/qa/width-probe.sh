#!/usr/bin/env bash
# width-probe.sh — sample one sandbox tab's bar width around two toggles.
#
# A diagnostic for a swap ask the bar logs and the tab does not honour
# (2026-09-22, the devbox's restored tabs). The bar logs only when it asks,
# so "the pane flashed narrow and came back" and "nothing happened" read the
# same in the log. This samples the tab's first tiled pane width from
# `dump-layout` every 150 ms across a toggle pair, through `ct.sh`, so it
# can only ever reach the sandbox.
#
#   scripts/qa/width-probe.sh <tab-name-fragment> <go-to-tab index> <since HH:MM:SS>
set -u
frag="$1"; idx="$2"; since="$3"
cd "$(dirname "$0")/../.." || exit 1
ct=scripts/ct.sh
w() {
  $ct dump-layout 2>/dev/null \
    | awk -v frag="$frag" '/tab name=/{f = index($0, frag) > 0} f && /pane size=/{print $2; exit}'
}
sample() {
  for _ in $(seq 1 "$1"); do
    echo "$(date +%T.%N | cut -c1-12) $(w)"
    sleep 0.15
  done
}
echo "focus tab $idx ($frag)"; $ct go-to-tab "$idx"; sleep 1
echo "width now: $(w)"
echo "toggle 1"; $ct pipe --name clave-toggle -- 1; sample 12
echo "toggle 2"; $ct pipe --name clave-toggle -- 1; sample 25
echo "== asks since $since"
log="/tmp/zellij-$(id -u)/zellij-log/zellij.log"
[ -f "$log" ] || log="$HOME/Library/Caches/org.Zellij-Contributors.Zellij/zellij-log/zellij.log"
grep "swap-width" "$log" | grep "$(date +%Y-%m-%d)" | awk -v t="$since" '$4 >= t' \
  | sed 's/^DEBUG  |[^|]*| //' | cut -c1-150
