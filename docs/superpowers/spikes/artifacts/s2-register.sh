#!/usr/bin/env bash
# S2 helper: a test pane registers its own pane_id under a given uuid, then
# drops to a shell so the pane stays alive and focusable.
# usage: s2-register.sh <uuid>
set -euo pipefail
UUID="${1:?usage: s2-register.sh <uuid>}"
echo "pane $ZELLIJ_PANE_ID registering as uuid=$UUID"
zellij pipe --name clave-register -- "{\"uuid\":\"$UUID\",\"pane_id\":$ZELLIJ_PANE_ID}"
exec "${SHELL:-/bin/zsh}"
