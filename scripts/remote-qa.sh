#!/usr/bin/env bash
# remote-qa.sh — the QA loop against a REMOTE machine, the same shape as the
# local one: stage, wait for the human's launch, drive, read the logs.
#
# Why (2026-09-22): the devbox surfaced a fleet of regressions no Mac sandbox
# could reproduce — the width flap needed `pane_frames false`, which the box
# has and the Mac does not, and the restore defects needed the box's Claude
# Code daemon. Every one of them was found by a human at the box and
# diagnosed by an agent reading the box's log over ssh. This script makes
# that loop the ordinary one: the agent runs the drive ON the box, over ssh,
# and every log it produces is readable from here.
#
# One code path (AGENTS.md): nothing here is a second drive. It pushes this
# checkout to a plain git repo on the remote, and runs the SAME `just qa`
# there, in the remote's own environment — its zellij config, its frames
# setting, its Claude Code. The remote sandbox is `clave dev instance` on the
# remote, keyed off the remote checkout dir like every sandbox; the remote's
# live fleet is untouched for the same reasons the local one is.
#
# The human's one job is unchanged: launch. The launch command is
# `ssh -t <host> 'cd <dir> && just launch'`, and it works from ANY terminal,
# inside zellij or not, because ssh does not forward the ZELLIJ variables that
# make `just launch` refuse.
#
# Subcommands:
#   sync              push HEAD to the remote checkout (creates it if absent)
#   qa [scenario]     sync, then run `just qa <scenario>` on the remote and
#                     stream it here; prints the launch command first
#   stage [scenario]  sync, then `just sandbox <scenario>` only
#   log [n]           the last n lines of the remote zellij log (default 40)
#   drive-log [n]     the last n lines of the newest remote drive log
#   instance          the remote sandbox's session name and root
#   kill              kill the remote SANDBOX session, by its exact name
#
# Env: CLAVE_QA_HOST (default devbox), CLAVE_QA_DIR (default code/clave-qa,
# relative to the remote $HOME).
set -euo pipefail

HOST="${CLAVE_QA_HOST:-devbox}"
DIR="${CLAVE_QA_DIR:-code/clave-qa}"
CMD="${1:-qa}"
[[ $# -gt 0 ]] && shift

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

# A non-interactive ssh shell may not carry the remote's cargo and just on
# PATH; `~/.cargo/env` puts cargo there and just is expected beside it.
remote() {
  ssh "$HOST" "cd \"\$HOME/$DIR\" 2>/dev/null; [ -f \"\$HOME/.cargo/env\" ] && . \"\$HOME/.cargo/env\"; $*"
}

# The remote checkout is a plain repo that accepts a push onto its checked-out
# branch (`receive.denyCurrentBranch updateInstead`), so one `git push` is the
# whole sync — no rsync, no scp, and a worktree's `.git` FILE (which rsync
# would copy as a dangling pointer, FOOTGUNS) never leaves this machine.
# The build tag on the remote is then this HEAD's short SHA, which is what
# the drive's preflight reads off the loaded bar.
sync_remote() {
  if ! ssh "$HOST" "git -C \"\$HOME/$DIR\" rev-parse --is-inside-work-tree" >/dev/null 2>&1; then
    echo "==> Creating the remote checkout $HOST:~/$DIR"
    ssh "$HOST" "git init -q -b main \"\$HOME/$DIR\" && git -C \"\$HOME/$DIR\" config receive.denyCurrentBranch updateInstead"
  fi
  echo "==> Pushing $(git rev-parse --short HEAD) to $HOST:~/$DIR"
  git push -q --force "ssh://$HOST/~/$DIR" HEAD:main
  remote 'git log --oneline -1'
}

launch_line() {
  cat <<EOF

==> The launch is YOURS. From any terminal (ssh forwards no zellij identity):

    ssh -t $HOST 'cd ~/$DIR && just launch'

EOF
}

case "$CMD" in
  sync)
    sync_remote
    ;;
  stage)
    sync_remote
    remote "just sandbox ${1:-c8-cold-start}"
    launch_line
    ;;
  qa)
    sync_remote
    SCENARIO="${1:-qa-fleet}"
    WAIT="${2:-1800}"
    launch_line
    # The remote drive tees its own log under the remote sandbox state dir;
    # `drive-log` reads it back. Streaming it here as well means a dropped
    # ssh loses nothing.
    remote "just qa $SCENARIO $WAIT"
    ;;
  log)
    remote "tail -n ${1:-40} /tmp/zellij-\$(id -u)/zellij-log/zellij.log"
    ;;
  drive-log)
    # `find`, not a glob: the remote login shell may be zsh, whose unmatched
    # glob is an error, not an empty list.
    remote 'f="$(find "$(./target/release/clave dev instance --field state)/qa" -name "drive-*.log" 2>/dev/null | sort | tail -1)"; if [ -n "$f" ]; then tail -n '"${1:-40}"' "$f"; else echo "no drive log yet"; fi'
    ;;
  instance)
    remote './target/release/clave dev instance --field session; ./target/release/clave dev instance --field root'
    ;;
  kill)
    # By exact name, read from the remote binary — never a pattern, never the
    # remote's live fleet (AGENTS.md: kill only the sandbox you asked for).
    remote 's="$(./target/release/clave dev instance --field session)"; zellij kill-session "$s"; zellij delete-session --force "$s"; echo "killed $s"'
    ;;
  *)
    echo "usage: $0 {sync|stage [scenario]|qa [scenario] [wait]|log [n]|drive-log [n]|instance|kill}" >&2
    exit 2
    ;;
esac
