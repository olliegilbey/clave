#!/usr/bin/env bash
# Spike S0 + S0b — does `claude --session-id <fresh-uuid>` CREATE a session
# jsonl, and does our munge_cwd() match Claude's on-disk projects/<dir> naming?
#
# WARNING: launches REAL Claude sessions (network + tokens). Run deliberately.
set -euo pipefail

PROJECTS="$HOME/.claude/projects"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
munge() { ( cd "$REPO" && cargo run -q -p clave --example munge -- "$1" ); }

# Three path shapes: plain, dotted, and a git worktree under .claude-worktrees.
ROOT="$(mktemp -d)/clave.spike"          # dotted segment forces the `.`→`-` rule
mkdir -p "$ROOT/plain" "$ROOT/dot.dir"
git init -q "$ROOT/base"
( cd "$ROOT/base" && git commit -q --allow-empty -m init )
git -C "$ROOT/base" worktree add -q "$ROOT/base/.claude-worktrees/wt" -b spike

echo "=== S0 + S0b: fresh-uuid create + munge-matches-disk ==="
for CWD in "$ROOT/plain" "$ROOT/dot.dir" "$ROOT/base/.claude-worktrees/wt"; do
  UUID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
  echo "-- cwd=$CWD  uuid=$UUID"
  # Primary: headless print mode. If it does not persist a jsonl, use the
  # interactive fallback documented in S0-S0b.md.
  ( cd "$CWD" && claude --session-id "$UUID" -p "reply with the single word: ok" >/dev/null 2>&1 ) \
    || echo "   warn: claude -p exited non-zero"
  DIR="$(munge "$CWD")"
  JSONL="$PROJECTS/$DIR/$UUID.jsonl"
  if [[ -f "$JSONL" ]]; then
    echo "   PASS created + munge matches: $JSONL"
  else
    echo "   FAIL not at computed path: $JSONL"
    echo "   where did the uuid actually land? ->"
    grep -rl "$UUID" "$PROJECTS" 2>/dev/null | sed 's#^#     #' || echo "     (nowhere — creation failed)"
  fi
done

echo
echo "=== S0: pre-existing-uuid behavior (resume vs error) ==="
UUID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
CWD="$ROOT/plain"; DIR="$(munge "$CWD")"; JSONL="$PROJECTS/$DIR/$UUID.jsonl"
( cd "$CWD" && claude --session-id "$UUID" -p "first message" >/dev/null 2>&1 ) || true
BEFORE=$(wc -l < "$JSONL" 2>/dev/null || echo 0)
echo "-- re-running with the SAME uuid:"
( cd "$CWD" && claude --session-id "$UUID" -p "second message" ); echo "   exit=$?"
AFTER=$(wc -l < "$JSONL" 2>/dev/null || echo NA)
echo "   jsonl lines before=$BEFORE after=$AFTER"
echo "   (grew ⇒ silently resumed; non-zero exit/error ⇒ collision is a hard error)"

echo
echo "Cleanup: rm -rf $ROOT   (leaves the ~/.claude/projects spike sessions for inspection)"
