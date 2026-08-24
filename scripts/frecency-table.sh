#!/usr/bin/env bash
# frecency-table.sh — READ-ONLY frecency explainer for the ambient store.
#
# One row per live tab (plus scored dormant rows with -a): the trailing seven
# day buckets as `count→contribution`, the weight each column carries at the
# store's half-life dial, and the resulting score, sorted as the bar sorts.
# The score is max(agent row, tab twin) per the comparator; a `*` after the
# tab id marks a row whose twin won. Usage:
#   scripts/frecency-table.sh [-a|--all] [path/to/agents.json]
set -euo pipefail

STORE="" ALL=0
while [ $# -gt 0 ]; do
  case "$1" in
  -a | --all) ALL=1 ;;
  *) STORE="$1" ;;
  esac
  shift
done
STORE="${STORE:-${CLAVE_STATE_DIR:-$HOME/.local/state/clave}/agents.json}"
TODAY=$(( $(date +%s) / 86400 ))
HL=$(jq -r '.order.frecency.half_life_hours // 24' "$STORE")
# A zero dial is reachable via CLI/wire; the bar clamps it to one hour
# (model.rs frecency_millis, `half_life_hours.max(1)`). Mirror that, or
# every weight below divides by zero.
if [ "$HL" -eq 0 ]; then HL=1; fi

# column -t drops empty fields (strtok semantics), so pad blanks with a space.
hdr="score\tblock\ttab\ttitle\trepo\tsummary"
wrow=" \t \t \t \t \thalf-life ${HL}h, weight ×"
for back in 0 1 2 3 4 5 6; do
  hdr+="\t$(date -r $(( (TODAY - back) * 86400 )) '+%a %-d')"
  wrow+="\t$(jq -n --argjson b "$back" --argjson hl "$HL" \
    'pow(0.5; $b * 24 / $hl) * 1000 | round / 1000')"
done

{
  printf '%b\n' "$hdr" "$wrow"
  jq -r --argjson today "$TODAY" --argjson hl "$HL" --argjson all "$ALL" '
    def sc(b): [ (b // {}) | to_entries[]
      | select((.key|tonumber) + 7 > $today)
      | .value * pow(0.5; ($today - (.key|tonumber)) * 24 / $hl) ] | add // 0;
    def f2: (. * 100 | round) / 100;
    def f1: (. * 10 | round) / 10;
    . as $s
    | [ .agents | to_entries[] | .value
        | ($s.tab_buckets[(.tab_id // -1) | tostring] // {}) as $twin
        | sc(.buckets) as $own | sc($twin) as $tw
        | { live: (.tab_id != null),
            tab: (.tab_id // "-"),
            title: ((.title // "") | if . == "" then "—" else .[0:12] end),
            repo: (.repo_root | split("/") | last),
            sum: ((.summary // "")
                  | if . == "" then "—" else . end
                  | .[0:28] | gsub("\t"; " ")),
            score: ([$own, $tw] | max),
            star: (if $tw > $own then "*" else "" end),
            win: (if $tw > $own then $twin else (.buckets // {}) end) }
        | select(.live or ($all == 1 and .score > 0)) ]
    | sort_by((if .live then 0 else 1 end), -.score, .tab)
    | .[]
    | [ (.score | f2), (if .live then "live" else "dorm" end),
        "\(.tab)\(.star)", .title, .repo, .sum ]
      + [ range(0; 7) as $back
          | (.win[($today - $back) | tostring] // 0) as $c
          | if $c == 0 then "·"
            else "\($c)→\($c * pow(0.5; $back * 24 / $hl) | f1)"
            end ]
    | @tsv' "$STORE"
} | column -t -s $'\t'
