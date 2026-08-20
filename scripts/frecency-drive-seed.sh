#!/usr/bin/env bash
# Seed mocked frecency buckets into THIS worktree's sandbox store (drive plan:
# docs/superpowers/plans/2026-08-19-frecency-e2e-drive.md). Idempotent; run
# after `just sandbox` (staging rewrites the store) and before launch.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLAVE_BIN="${CLAVE_BIN:-$ROOT/target/release/clave}"
STATE="$("$CLAVE_BIN" dev instance --field state)"
STORE="$STATE/agents.json"
T=$(( $(date +%s) / 86400 ))
A=00000000-0000-4000-8000-c85c00000001
B=00000000-0000-4000-8000-c85c00000002
C=00000000-0000-4000-8000-c85c00000003
jq --argjson t "$T" "
  .agents[\"$A\"].buckets = {(\$t-6|tostring):8, (\$t-3|tostring):8, (\$t-1|tostring):8, (\$t|tostring):2} |
  .agents[\"$A\"].summary = \"invested\" |
  .agents[\"$B\"].buckets = {(\$t|tostring):1} |
  .agents[\"$B\"].summary = \"one-off today\" |
  .agents[\"$B\"].commit_ord = 99 |
  .agents[\"$C\"].buckets = {(\$t-6|tostring):30} |
  .agents[\"$C\"].summary = \"dormant giant\" |
  .seq = 100
" "$STORE" > "$STORE.tmp"
mv "$STORE.tmp" "$STORE"
echo "seeded (today=$T):"
jq -c '.agents | to_entries[] | {slug: .value.summary, ord: .value.commit_ord, buckets: .value.buckets}' "$STORE"
