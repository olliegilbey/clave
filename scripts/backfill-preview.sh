#!/usr/bin/env bash
# backfill-preview.sh — READ-ONLY preview of what the version-refresh bucket
# backfill would seed for the ambient store (jsonl-source-of-truth round,
# 2026-08-20). Mirrors backfill.rs: genuine user turns (not meta, not
# sidechain, no tool_result block) per unix day, trailing-7-day window,
# empty-bucket rows only. Writes nothing anywhere.
set -euo pipefail
STATE="${CLAVE_STATE_DIR:-$HOME/.local/state/clave}"
CLAUDE="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"
TODAY=$(( $(date +%s) / 86400 ))

jq -r '.agents | to_entries[]
  | select((.value.buckets // {}) | length == 0)
  | [.key, (.value.live_session // .key), .value.cwd, .value.repo_root] | @tsv' \
  "$STATE/agents.json" |
while IFS=$'\t' read -r uuid session cwd root; do
  for dir in "$cwd" "$root"; do
    f="$CLAUDE/projects/$(echo "$dir" | tr '/.' '--')/$session.jsonl"
    [[ -f "$f" ]] && break
    f=""
  done
  # Mirror backfill.rs's by-name scan: a session id is globally unique, so
  # `projects/*/<session>.jsonl` wherever it lives IS the transcript.
  if [[ -z "$f" ]]; then
    f=$(command ls "$CLAUDE/projects/"*/"$session.jsonl" 2>/dev/null | head -1 || true)
  fi
  if [[ -z "$f" ]]; then
    echo "$uuid  NO TRANSCRIPT (stays ordinal-fallback)"
    continue
  fi
  buckets=$(jq -cs --argjson today "$TODAY" '
    [ .[] | select(.type=="user")
      | select((.isMeta // false) | not) | select((.isSidechain // false) | not)
      | select((.message.content | type) == "string"
               or ((.message.content | type) == "array"
                   and ([.message.content[]? | select(.type=="tool_result")] | length) == 0))
      | .timestamp | select(. != null)
      | (sub("\\..*Z$"; "Z") | fromdateiso8601 / 86400 | floor)
      | select(. + 7 > $today and . <= $today) ]
    | group_by(.) | map({(first|tostring): length}) | add // {}' "$f")
  echo "$uuid  $buckets"
done
