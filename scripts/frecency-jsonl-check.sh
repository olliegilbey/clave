#!/usr/bin/env bash
# frecency-jsonl-check.sh — READ-ONLY consistency check: for every store row
# carrying buckets, recompute the day-buckets from its jsonl transcript
# (same genuine-user-turn filter as backfill.rs / backfill-preview.sh) and
# print store vs jsonl side by side with both frecency scores (24h half-life).
set -euo pipefail
STATE="${CLAVE_STATE_DIR:-$HOME/.local/state/clave}"
CLAUDE="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"
TODAY=$(( $(date +%s) / 86400 ))

jq -r '.agents | to_entries[]
  | select((.value.buckets // {}) | length > 0)
  | [.key, (.value.live_session // .key), .value.cwd, .value.repo_root,
     (.value.tab_id // "-"), (.value.buckets | tojson),
     (.value.summary // .value.title // "" | .[0:32])] | @tsv' \
  "$STATE/agents.json" |
while IFS=$'\t' read -r uuid session cwd root tab store_buckets summary; do
  for dir in "$cwd" "$root"; do
    f="$CLAUDE/projects/$(echo "$dir" | tr '/.' '--')/$session.jsonl"
    [[ -f "$f" ]] && break
    f=""
  done
  if [[ -z "$f" ]]; then
    f=$(command ls "$CLAUDE/projects/"*/"$session.jsonl" 2>/dev/null | head -1 || true)
  fi
  if [[ -z "$f" ]]; then
    echo "NO-TRANSCRIPT tab=$tab store=$store_buckets  $summary"
    continue
  fi
  # -Rs + fromjson?: backfill.rs skips malformed transcript lines (its
  # `let Ok(v) = … else continue`); a slurp that aborts on one bad line
  # would diverge from the thing this script exists to mirror.
  jsonl_buckets=$(jq -Rsc --argjson today "$TODAY" '
    [ split("\n")[] | fromjson? ]
    | [ .[] | select(.type=="user")
      | select((.isMeta // false) | not) | select((.isSidechain // false) | not)
      | select((.message.content | type) == "string"
               or ((.message.content | type) == "array"
                   and ([.message.content[]? | select(.type=="tool_result")] | length) == 0))
      | .timestamp | select(. != null)
      | (sub("\\..*Z$"; "Z") | fromdateiso8601 / 86400 | floor)
      | select(. + 7 > $today and . <= $today) ]
    | group_by(.) | map({(first|tostring): length}) | add // {}' "$f")
  score() { jq -nr --argjson b "$1" --argjson today "$TODAY" \
    '[ $b | to_entries[] | .value * pow(0.5; ($today - (.key|tonumber))) ] | add // 0'; }
  s_store=$(score "$store_buckets")
  s_jsonl=$(score "$jsonl_buckets")
  match=$([ "$store_buckets" = "$jsonl_buckets" ] && echo OK || echo DIFF)
  printf '%s tab=%-2s score store=%-6s jsonl=%-6s store=%s jsonl=%s  %s\n' \
    "$match" "$tab" "$s_store" "$s_jsonl" "$store_buckets" "$jsonl_buckets" "$summary"
done
