#!/usr/bin/env bash
# width-probe-cli.sh — provoke a bar's own width ask WITHOUT a toggle.
#
# Companion to width-probe.sh (2026-09-22). Focus one sandbox tab, move it
# by CLI swap to the width the store's mode wants (the bar rests), then move
# it by CLI swap to the other width: the focused bar now disagrees with the
# store and must ask. Sample the tab's first tiled pane width every 150 ms
# across that ask, so a swap that lands and is undone shows as a blip.
#
#   scripts/qa/width-probe-cli.sh <tab-name-fragment> <go-to-tab index> <since HH:MM:SS>
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
echo "cli previous-swap-layout (1)"; $ct previous-swap-layout; sample 10
echo "cli previous-swap-layout (2)"; $ct previous-swap-layout; sample 30
echo "== asks since $since"
log="/tmp/zellij-$(id -u)/zellij-log/zellij.log"
[ -f "$log" ] || log="$HOME/Library/Caches/org.Zellij-Contributors.Zellij/zellij-log/zellij.log"
grep "swap-width" "$log" | grep "$(date +%Y-%m-%d)" | awk -v t="$since" '$4 >= t' \
  | sed 's/^DEBUG  |[^|]*| //' | cut -c1-150
