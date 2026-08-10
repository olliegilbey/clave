#!/usr/bin/env bash
# ct.sh — run a zellij action against the clave-test SANDBOX, or refuse.
#
# Why this exists (2026-08-07): `ZELLIJ_SESSION_NAME=clave-test zellij action …`
# reads like a boundary and is not one. An agent shell inside the maintainer's
# session also inherits `ZELLIJ` and `ZELLIJ_PANE_ID`; override the name alone
# and it works right up until `clave-test` stops existing — at which point the
# CLI FALLS BACK to the ambient session instead of erroring. A whole queued
# drive then lands on the daily fleet. It did: a sandbox-built debug bar became
# a real pane in the maintainer's live session, and ten collapse toggles went to
# his fleet.
#
# The mechanism, so the next reader can check it rather than trust it
# (`src/commands.rs:407-452`, tag v0.44.3): with no `--session` flag,
# `send_action_to_session` receives `requested_session_name = None`, and the
# `ActiveSession::One` arm then serves the ONLY live session, whatever
# `ZELLIJ_SESSION_NAME` says. The flag closes that: with a name supplied, both
# the `One` and `Many` arms exit 1 when it does not match a live session. So
# `exec` below passes `--session` and never relies on the env at all — the
# `export` is belt to that brace.
#
# So this wrapper fails CLOSED. It proves the sandbox is live, clears the
# ambient session, and only then runs. Use it for every drive command:
#
#     scripts/ct.sh new-tab --name t1
#     scripts/ct.sh start-or-reload-plugin "file:$SANDBOX/clave-bar.wasm" -c clave_binary=clave
#
# It deliberately does NOT accept a session name. There is exactly one session
# an agent may drive, and making it an argument would make the mistake
# expressible again.
set -euo pipefail

# ASKED, NEVER ASSUMED. This was `SESSION="clave-test"` — one name for one
# machine-wide sandbox — and #161 replaced that with a per-agent instance keyed
# on the worktree directory. A hardcoded name survives that change looking
# perfectly fine from the main checkout and targets SOMEONE ELSE'S sandbox from
# a worktree, which is the class of mistake this wrapper exists to make
# unexpressible. The binary is the single source of the derivation, exactly as
# `sandbox-setup.sh` treats it, so the shell and the CLI cannot disagree.
#
# Fails closed on its own terms: if `clave dev instance` cannot name a session
# — no `clave` on PATH, an unkeyable worktree — there is no safe fallback,
# because the fallback IS the bug. Refuse and say why.
# ASK THIS CHECKOUT'S BINARY, NOT WHATEVER `clave` MEANS TODAY. Bare `clave` on
# PATH is the maintainer's STABLE install — that is "the one leak" (#43/#44),
# and it bit here immediately: the stable v0.1.2 has no `dev instance`
# subcommand at all, so asking it produced an error, and a wrapper that treats
# any failure as "refuse" then refuses forever for a reason that reads like "the
# sandbox is down". The script ships inside a checkout; that checkout's build is
# the authority on which sandbox the checkout owns, and it is the same binary
# `just sandbox` staged from.
CLAVE_BIN="${CLAVE_BIN:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/target/release/clave}"
if [[ ! -x "$CLAVE_BIN" ]]; then
  cat >&2 <<EOF
REFUSING: no built clave at
  ${CLAVE_BIN}

The wrapper asks THIS checkout's binary which sandbox it owns, because bare
\`clave\` on PATH is the stable install and answers for the wrong one. Build it
(\`just sandbox\` does, on the way in), or set \$CLAVE_BIN.
EOF
  exit 1
fi

if ! SESSION="$("$CLAVE_BIN" dev instance --field session 2>/dev/null)" || [[ -z "$SESSION" ]]; then
  cat >&2 <<EOF
REFUSING: could not resolve this checkout's sandbox session.

\`${CLAVE_BIN} dev instance --field session\` produced nothing — this worktree's
name cannot key an instance. There is deliberately no fallback: guessing a
session name is how a drive reaches the maintainer's fleet.
EOF
  exit 1
fi

# `$TMPDIR` CARRIES A TRAILING SLASH on macOS, and zellij's own path does not:
# the server's argv reads `…/T/zellij-501/…` while the naive interpolation here
# gives `…/T//zellij-501/…`. The `-S` test does not care — the kernel folds the
# double slash — but the `pgrep` below matches argv as TEXT, so it never
# matched and this wrapper refused every command on every macOS machine. It
# fails closed, so it looked exactly like "the sandbox is not running".
# (Found reviewing PR #152, 2026-08-10.)
TMP="${TMPDIR:-/tmp}"
SOCKET_ROOT="${TMP%/}/zellij-$(id -u)"

if [[ $# -eq 0 ]]; then
  echo "usage: $0 <zellij-action> [args…]   (runs against ${SESSION} only)" >&2
  exit 2
fi

# The socket directory is keyed on zellij's client/server contract version, so
# glob rather than hard-code it: a contract bump must turn this into a wrapper
# that says so, not one that silently refuses everything for a version reason.
#
# Collect ALL of them and demand exactly one (CodeRabbit, PR #152). What is
# checked here is a socket PATH; what runs below is resolved by session NAME,
# and two contract directories each holding a `clave-test` would make those two
# different things. Zellij would pick for itself and this preflight would have
# vouched for the other one. Refusing is right: a machine in that state has a
# leftover from another zellij version, and the answer is to clean it up, not
# to guess.
SOCKETS=()
for candidate in "${SOCKET_ROOT}"/contract_version_*/"${SESSION}"; do
  [[ -S "$candidate" ]] && SOCKETS+=("$candidate")
done

# 1. The socket must exist. A dead session leaves nothing here, which is
#    precisely the case that used to fall through to the maintainer's fleet.
if [[ ${#SOCKETS[@]} -eq 0 ]]; then
  cat >&2 <<EOF
REFUSING: no ${SESSION} session socket under
  ${SOCKET_ROOT}/contract_version_*/${SESSION}

The sandbox is not running, so this command would have been served by whatever
session your shell is attached to — the maintainer's fleet. Ask him to launch
the sandbox (staging is 'just sandbox'; launching is his), then retry.
EOF
  exit 1
fi

if [[ ${#SOCKETS[@]} -gt 1 ]]; then
  echo "REFUSING: ${#SOCKETS[@]} '${SESSION}' sockets exist, under different zellij" >&2
  echo "contract versions. Which one a client picks is zellij's choice, not this" >&2
  echo "script's, so the preflight below could vouch for the wrong server:" >&2
  printf '  %s\n' "${SOCKETS[@]}" >&2
  echo "Remove the stale contract directory, then retry." >&2
  exit 1
fi

SOCKET="${SOCKETS[0]}"

# 2. A stale socket file outlives its server. Require a live server process
#    holding this exact path, so a leftover socket cannot green the check.
if ! pgrep -f "zellij --server ${SOCKET}" >/dev/null 2>&1; then
  echo "REFUSING: socket exists but no 'zellij --server ${SOCKET}' process is alive." >&2
  echo "That is a stale socket from a dead sandbox — treat it as not running." >&2
  exit 1
fi

# 3. Clear the ambient session so there is nothing to fall back to even if a
#    future zellij changes how it resolves a target. Belt as well as braces:
#    the name is set explicitly AND the inherited context is gone.
unset ZELLIJ ZELLIJ_PANE_ID
export ZELLIJ_SESSION_NAME="$SESSION"

# 4. Bound it (CodeRabbit, PR #152). Every check above races with the session
#    dying, and `zellij action` against a dead or wedged session BLOCKS
#    INDEFINITELY AND NEVER ERRORS (FOOTGUNS) — which is the single worst thing
#    to hand an autonomous loop, because the agent has no signal at all and the
#    human sees a hang. A wall clock is the only guard against a race a preflight
#    cannot win. `action` does not stream stdin, so unlike `zellij pipe` a killed
#    client here leaves nothing half-open in the server's CLI lane.
#
#    Nothing normal takes seconds; override for a deliberately slow action.
CT_TIMEOUT="${CLAVE_CT_TIMEOUT:-15}"

if command -v timeout >/dev/null 2>&1; then
  exec timeout "$CT_TIMEOUT" zellij --session "$SESSION" action "$@"
elif command -v gtimeout >/dev/null 2>&1; then
  exec gtimeout "$CT_TIMEOUT" zellij --session "$SESSION" action "$@"
else
  # No coreutils. `alarm` survives `exec` (POSIX: pending alarms are not reset
  # by an exec), and SIGALRM's default action terminates — so this bounds the
  # real client, not a wrapper. Exits 142 on expiry.
  exec perl -e 'alarm shift @ARGV; exec @ARGV or die "exec failed: $!\n"' \
    "$CT_TIMEOUT" zellij --session "$SESSION" action "$@"
fi
