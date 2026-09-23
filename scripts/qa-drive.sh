#!/usr/bin/env bash
# qa-drive.sh — the automated regression drive, phases 0-7 (docs/dev/QA-DRIVE.md).
#
# What this is: preflight, baseline join, the dormant-row bind ladder, tab
# churn, the nav ring walk, the collapse burst, the card cells, the terminal
# facts, quiescence, the isolation witness, the relaunch and the teardown
# hand-back, scripted against THIS checkout's per-worktree sandbox instance.
#
# ONE phase asks the maintainer for a second thing: 6c wants the sandbox quit
# and launched again, and waits for both. Everything else runs unattended once
# the first launch is up.
#
# The instrument it reads with — tracing, assertions, and the zellij log read
# per BAR INSTANCE — is `scripts/qa/lib.sh`, tested offline by
# `scripts/qa/lib-selftest.sh` (gated: `crates/clave/tests/qa_lib.rs`).
#
# ALL PHASES DRIVEN LIVE GREEN — run 4, 2026-08-17, full 0-7 pass plus both
# human eyeball checkpoints; run 11, 2026-09-11, all TEN phases (0-7 with
# 5b card-cells and 6b isolation-witness), first run of the `just qa` loop;
# and run 22, 2026-09-17, all TWELVE phases, 237 checks, under the live-set
# restore that was removed on 2026-09-22 (6c now proves one eager tab and
# the rows left live on standby, the rest dormant). The list below was the FIRST LIVE RUN PENDING
# ledger; it is kept because each entry records an assumption a live run had
# to settle, and how the first runs settled them: runs 1-3 each went red on a
# real finding first (the stale-executor nav wedge, the starved-bar prune of
# a newborn bind — both fixed in clave-bar — and wait_collapsed's jq `//`
# blindness to `false`, fixed here). Per-check markers remain at their sites:
#   (1) `go-to-tab-by-id` exists on the maintainer's zellij server (0.44.3 has
#       it; an older server takes the positional fallback, which is verified);
#   (2) the focused tab is read from `dump-layout`'s `focus=true` tab node and
#       joined back to a tab_id by RANK — the rank join is base-independent,
#       but the dump's tab order is assumed to be tab-position order;
#   (3) `clave prune-tabs` lands its store echo inside the 15s re-join window;
#   (4) zellij recycles the closed highest tab id onto the next `new-tab` in
#       THIS session (screen.rs `get_new_tab_id`) — recorded, and the stamp
#       assertions say so honestly when it does not hold;
#   (5) the ring's landing prediction (rendered dormant order) matches what the
#       bar actually renders. That prediction IS phase 4's assertion, so a
#       mismatch is a finding to read, not a script bug to assume;
#   (6) the collapse burst assumes CLI pipes deliver serialized — five rapid
#       `clave-toggle` presses land as five presses on ONE writer, so the
#       store's final parity is the burst's witness (pipe.rs pins the twin
#       guard; nothing pins CLI delivery order but the queue itself). Runs
#       1-4 drove this with a SERIAL loop — the CLI pipe blocks until the
#       plugin unblocks it, so no queue ever formed; the burst launches
#       concurrently since the PR #202 review, and the queued shape has not
#       yet been driven live;
#   (7) quiescence assumes the seeded fleet is hook-quiet at rest — a seeded
#       agent that still ticks (an unfinished claude -p, a background
#       SessionEnd) advances `seq` under the flat-line check and reads as a
#       false red. The check prints both readings so that shape is legible.
#   (8) phase 5c's leg A assumes `write-chars` reaches the focused pane of the
#       FOCUSED TAB (it is a client-level action, so it should), and that a
#       `sleep` started there reads back as a running foreground command. If
#       leg A goes red, read the pane before the bar: an unreached keystroke
#       and an undelivered fact look identical in the log, which is why the
#       phase prints the pane's own process first;
#   (9) phase 5c's leg B is the OPEN question, not an assumption — whether
#       `get_pane_cwd` answers for a pane in a non-active tab. It is measured
#       every run and asserted never, and its answer decides the shape of the
#       store-backed fix.
#
# What this is NOT: a launcher. It assumes a human has ALREADY staged
# (`just sandbox <scenario>`) and LAUNCHED the sandbox session. It never runs
# `zellij kill-session`/`delete-session`/`new-session` — session lifecycle
# stays the human's (AGENTS.md, TESTING.md "the interaction contract"). Every
# zellij touch goes through `scripts/ct.sh`, the one sanctioned wrapper, which
# refuses closed if the instance session is not live rather than falling back
# to whatever session the caller's shell happens to be inside.
#
# Tracing: every phase/check/measure line is teed into
# <state-dir>/qa/drive-<epoch>.log — never discarded, never /dev/null, per
# the QA-DRIVE tracing spec. "empty" is printed as the word so a silent
# failure and a clean pass never look alike.
#
# Usage: scripts/qa-drive.sh <scenario>
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CT="$SCRIPT_DIR/ct.sh"
# Sandbox derivation is cwd-keyed (sandbox.rs): run from THIS checkout's
# root so `dev instance` resolves THIS worktree's sandbox even when the
# script is invoked by absolute path from some other directory.
cd "$ROOT" || exit 1
CLAVE_BIN="${CLAVE_BIN:-$ROOT/target/release/clave}"

usage() {
  cat <<EOF
usage: $0 <scenario>

Drives QA-DRIVE phases 0-7 (preflight, baseline join, bind ladder, tab churn,
ring walk, collapse burst, card cells, terminal facts, quiescence, isolation
witness, relaunch, teardown hand-back) against THIS checkout's per-worktree sandbox
instance (\`clave dev instance\`). Never launches or kills a zellij session.

Phase 6c asks you to quit the sandbox and launch it again, then reads what
came back. It prints the commands and waits up to QA_RELAUNCH_WAIT seconds
(default 1800) for each half.

USUALLY YOU WANT: \`just qa <scenario>\` — it stages, prints the launch line,
waits for the human to run it, and then calls this script. One command for the
whole loop.

This script alone assumes the session is ALREADY staged and launched:

  just sandbox <scenario>
  (human, non-zellij terminal) cd <worktree> && just launch

and refuses closed if the instance's sandbox session is not live. Set
QA_WAIT_SECS=<n> to wait for the launch instead of refusing.

NOT IDEMPOTENT, by design: phase 2 rung 1 mints a row and the middle phases
consume the fleet shape they assert against, so a re-run needs a fresh stage.
Phase 1 says so plainly when the stage is stale instead of failing a count
that is correctly reading the previous run.

Isolation: this script scrubs the inherited zellij identity before its first
phase, so every child aims at the sandbox rather than at whatever session the
calling terminal sits inside. Fire hooks ONLY through \`ct.sh --hook\` —
\`crates/clave/tests/script_hygiene.rs\` fails the build otherwise, and
\`clave hook\` itself refuses a push whose target does not own the store it
wrote. All three exist because a hand-rolled hook call hung the maintainer's
live session on 2026-09-11 (FOOTGUNS #281).

  <scenario>   the scenario name already staged/launched (e.g. qa-fleet).
               Informational for the report header and for phase 1's exact
               row-count expectation, which is currently known for
               \`qa-fleet\` only — other scenarios still run every phase but
               phase 1's row-count checks fall back to measurement-only.
               Phases 3-4 need a fleet of at least three live tabs and a
               dormant block of at least two rows; they refuse with the
               measured counts rather than assert against a fleet too small
               to carry the property.
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ $# -ne 1 ]]; then
  usage >&2
  exit 2
fi

SCENARIO="$1"

# ---------------------------------------------------------------------------
# Preconditions — fail closed, same idiom as ct.sh: refuse with a clear
# message rather than guess or fall back.
# ---------------------------------------------------------------------------

if [[ ! -x "$CLAVE_BIN" ]]; then
  cat >&2 <<EOF
REFUSING: no built clave at
  ${CLAVE_BIN}

Build it (\`just sandbox\` does, on the way in), or set \$CLAVE_BIN.
EOF
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "REFUSING: jq is required and not on PATH." >&2
  exit 1
fi

# Ask THIS checkout's binary which sandbox it owns (see ct.sh's own
# rationale) — never guess a session/state/data dir, because guessing is
# exactly how a drive reaches the wrong instance.
if ! SESSION="$("$CLAVE_BIN" dev instance --field session 2>/dev/null)" || [[ -z "$SESSION" ]]; then
  cat >&2 <<EOF
REFUSING: could not resolve this checkout's sandbox session.

\`${CLAVE_BIN} dev instance --field session\` produced nothing — this
worktree's name cannot key an instance. There is deliberately no fallback.
EOF
  exit 1
fi
STATE_DIR="$("$CLAVE_BIN" dev instance --field state 2>/dev/null)"
DATA_DIR="$("$CLAVE_BIN" dev instance --field data 2>/dev/null)"
if [[ -z "$STATE_DIR" || -z "$DATA_DIR" ]]; then
  echo "REFUSING: could not resolve this instance's state/data dirs." >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# THE AMBIENT IDENTITY — scrubbed ONCE, here, for every child this drive will
# ever spawn.
# ---------------------------------------------------------------------------
# A drive shell runs INSIDE the maintainer's fleet, so it inherits
# `ZELLIJ_SESSION_NAME=<his session>` along with `ZELLIJ`/`ZELLIJ_PANE_ID`,
# and anything that consults them aims there. `ct.sh` scrubs them per
# invocation — which protects what goes through it, and nothing else. That
# gap is not theoretical: `clave hook` also PUSHES, aimed by
# `ZELLIJ_SESSION_NAME` (hook.rs `own_session`), so a hand-rolled hook call in
# phase 5b wrote the sandbox store and pushed at HIS bar, and hung his session
# (2026-09-11; FOOTGUNS #281, which had already named this exact trap).
#
# The lesson taken is that a convention every call site must remember is the
# wrong shape for this hazard. So the hostile variables do not survive this
# line, and the sandbox's own identity replaces them: every child — routed,
# hand-rolled, or added by someone who never read this comment — inherits the
# SANDBOX. The default becomes fail-safe instead of fail-dangerous.
#
# `ct.sh` keeps its own per-call scrub regardless: it is also run directly,
# and belt-as-well-as-braces is the house style for this particular hazard.
FOREIGN_SESSION="${ZELLIJ_SESSION_NAME:-}"
unset ZELLIJ ZELLIJ_PANE_ID
export ZELLIJ_SESSION_NAME="$SESSION"
# The store too, for the same reason: a child that resolves its own paths
# must land in the sandbox even when the caller passed it nothing.
export CLAVE_SESSION="$SESSION"
export CLAVE_STATE_DIR="$STATE_DIR"
export CLAVE_DATA_DIR="$DATA_DIR"

# The tripwire. Cheap, and it is a REORDERING guard: everything below assumes
# the scrub already happened, so if a future edit moves a phase above this
# point — or re-exports the inherited name — the drive stops instead of
# driving the wrong session.
if [[ "${ZELLIJ_SESSION_NAME:-}" != "$SESSION" || -n "${ZELLIJ:-}" || -n "${ZELLIJ_PANE_ID:-}" ]]; then
  cat >&2 <<EOF
REFUSING: the ambient zellij identity is not this sandbox.

  ZELLIJ_SESSION_NAME=${ZELLIJ_SESSION_NAME:-<unset>}  (want: ${SESSION})
  ZELLIJ=${ZELLIJ:-<unset>}  ZELLIJ_PANE_ID=${ZELLIJ_PANE_ID:-<unset>}  (want both unset)

Every child of this drive inherits these, and a push aimed by them reaches
whichever bar they name. Refusing rather than driving someone else's session.
EOF
  exit 1
fi
if [[ -n "$FOREIGN_SESSION" && "$FOREIGN_SESSION" != "$SESSION" ]]; then
  printf '==> scrubbed an inherited zellij identity: %s -> %s (every child now aims at the sandbox)\n' \
    "$FOREIGN_SESSION" "$SESSION"
fi

# How many pushes clave REFUSED to misaim, counted from the sandbox's own
# event log. `clave hook` decides a push's destination from the store it
# wrote rather than from the env (hook.rs `aim_push`) and writes one
# `push-refused` line when the two disagree — so this number is the drive's
# witness that nothing it spawned reached a bar it had no business reaching.
# Zero is the expected reading now that the identity is scrubbed above: a
# refusal means the guard caught something the scrub did not, which is a
# finding about this script, not a pass.
count_push_refusals() {
  local n
  n="$(grep -c '"cmd":"push-refused"' "$STATE_DIR/clave.log" 2>/dev/null)" || n=0
  printf '%s' "${n:-0}"
}

# `clave dev status` is liveness-gated by construction (TESTING.md, "the
# observability map") — safe to call even against a dead session, unlike a
# bare `zellij action`, which blocks indefinitely against one. This is the
# fail-closed refusal: the drive never proceeds against a session that is
# not actually up.
read_liveness() {
  STATUS_JSON="$("$CLAVE_BIN" dev status 2>/dev/null)" || STATUS_JSON=""
  SESSION_LIVE="$(printf '%s' "$STATUS_JSON" | jq -r '.session_live // false' 2>/dev/null)"
}
read_liveness

# WAITING for the launch, when asked to. The drive still never launches
# anything — session lifecycle stays the human's — it just stops refusing
# INSTANTLY, which is what made the loop three messages wide: stage, ask, wait
# to be told, drive. With `QA_WAIT_SECS` set (`just qa` sets it) the drive
# prints the launch command itself and blocks until the session appears, so
# the loop is one command for the agent and one for the human, concurrently.
QA_WAIT_SECS="${QA_WAIT_SECS:-0}"
if [[ "$SESSION_LIVE" != "true" && "$QA_WAIT_SECS" -gt 0 ]]; then
  cat <<EOF

==> Waiting up to ${QA_WAIT_SECS}s for '${SESSION}'. Launch it YOURSELF, in a
    NEW terminal window OUTSIDE zellij:

    cd ${ROOT}
    just launch

    That derives everything — session, state and data dirs, and the PATH shim
    that makes a bare \`clave\` resolve to THIS build. It refuses if run from
    inside a zellij session, which is why it is yours and not the agent's.

    The drive starts by itself the moment the session is up.

EOF
  QA_WAITED=0
  while [[ "$SESSION_LIVE" != "true" && "$QA_WAITED" -lt "$QA_WAIT_SECS" ]]; do
    sleep 2
    QA_WAITED=$((QA_WAITED + 2))
    read_liveness
  done
  [[ "$SESSION_LIVE" == "true" ]] && printf '==> '"'"'%s'"'"' is up after %ss — driving.\n\n' "$SESSION" "$QA_WAITED"
fi

if [[ "$SESSION_LIVE" != "true" ]]; then
  cat >&2 <<EOF
REFUSING: sandbox session '${SESSION}' is not live.

This drive never launches a session — stage and launch first:
  just sandbox ${SCENARIO}
  clave dev scenario ${SCENARIO}   # if not already seeded by \`just sandbox\`
  (human, non-zellij terminal) cd ${ROOT} && just launch
then re-run: $0 ${SCENARIO}

Or drive the whole loop in one command, which waits for the launch:
  just qa ${SCENARIO}
EOF
  exit 1
fi

# ---------------------------------------------------------------------------
# Log setup — every line from here on is teed, never discarded.
# ---------------------------------------------------------------------------

QA_DIR="$STATE_DIR/qa"
mkdir -p "$QA_DIR"
DRIVE_LOG="$QA_DIR/drive-$(date +%s).log"
exec > >(tee -a "$DRIVE_LOG") 2>&1
# `wait` at exit: `tee`'s process-substitution subshell is asynchronous, so
# without waiting for it the script can exit before the last lines flush —
# exactly the kind of discarded-output trap this drive exists to avoid.
trap 'wait' EXIT

echo "QA drive — scenario=${SCENARIO} session=${SESSION}"
echo "state=${STATE_DIR} data=${DATA_DIR}"
echo "log=${DRIVE_LOG}"

# `$TMPDIR` carries a trailing slash on macOS; zellij's own paths do not
# (FOOTGUNS, "Process and tooling" — the same normalisation ct.sh and
# seek-trace.sh both apply).
TMP="${TMPDIR:-/tmp}"
ZLOG="${TMP%/}/zellij-$(id -u)/zellij-log/zellij.log"

# The build tag `just sandbox` baked (sandbox-setup.sh derives it from
# `git rev-parse --short HEAD` in the checkout that staged it — same
# derivation here, from THIS checkout).
#
# HEAD_TAG is what THIS checkout is at now; BUILD_TAG is what the RUNNING
# wasm says it is. They start equal, and P0 below reconciles BUILD_TAG down
# to the loaded tag when HEAD has moved on host/docs only. Everything that
# greps the zellij log for a bar instance (scripts/qa/lib.sh) must use
# BUILD_TAG, or it looks for a tag no bar ever printed.
HEAD_TAG="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo dev)"
BUILD_TAG="$HEAD_TAG"

# The instrument: tracing, assertions, and the zellij log read per bar
# instance (scripts/qa/lib.sh — see its header for what it expects set, all
# of which is above it). Sourced rather than inlined so the log parsing can be
# tested without a launched session: scripts/qa/lib-selftest.sh.
# shellcheck source=scripts/qa/lib.sh
source "$SCRIPT_DIR/qa/lib.sh"

# The mark: everything phase 1+ reads from the zellij log is lines AFTER
# this point, taken at THIS script's start. Phase 0's build-tag check is the
# deliberate exception — see the comment at that check.
LOGMARK="$(zlog_now)"

# ===========================================================================
# Phase 0 — preflight
# ===========================================================================
phase "P0-preflight"

# Build tag on the loaded wasm tail. Deliberately UNMARKED — same reasoning
# as TESTING.md's sandbox-drive-loop step 3: the human's launch (and the
# load it caused) happened BEFORE this script's own mark, since the script
# assumes launch already happened, so a mark-filtered read here would see
# nothing. The proven mechanism is the TAIL of "clave-bar: loaded" lines,
# not a mark — see that step's "Do NOT grep -c for your tag" note.
#
# Undisclosed-until-now residual: the zellij log is shared across sessions
# and never truncated, so a re-run at the SAME HEAD — exactly what testing
# an uncommitted fix looks like — can leave an OLDER line carrying the same
# build tag on the tail if the newest load actually failed. A tag-string
# match alone cannot tell "fresh load, same tag" from "stale line, same
# tag, load silently failed". TESTING.md's human loop defuses this by
# eyeballing timestamps; teaching this check to parse them is out of scope
# (KISS). Mitigation: print the matched line verbatim plus its tail
# context every run, and disclose the gap explicitly via the NOTE below —
# never claim a certainty this check cannot back.
#
# And the log is CROSS-SESSION (see count_eof_twins / FOOTGUNS): a tab
# opened in the maintainer's live fleet DURING this preflight appends a
# release-tagged loaded line, so "the very last line is ours" is a race.
# The check therefore accepts the tag anywhere in the 5-line tail — still
# tail-bounded, never the forbidden whole-file grep -c.
LOADED_LINES="$(grep -F 'clave-bar: loaded' "$ZLOG" 2>/dev/null || true)"
LOADED_TAIL="$(printf '%s\n' "$LOADED_LINES" | tail -5)"
measure "loaded-tail (last 5)" "$LOADED_TAIL"
TAIL_MATCH="$(printf '%s\n' "$LOADED_TAIL" | grep -F "build=$HEAD_TAG" | tail -1)"

# An EXACT tag match is the happy path and the only one that needs no
# argument. But the property this check exists for is "the bar now running
# was built from the bar source I am testing", and HEAD moves for reasons the
# bar does not care about: commit a host-only fix, or a doc, and the loaded
# wasm is suddenly 'stale' by tag while being byte-identical in source. That
# false red used to mean a kill and a relaunch to clear, which is a strong
# incentive not to commit mid-drive — the wrong incentive entirely.
#
# So an older tag is accepted ONLY when git says the source it was built from
# is the same source: the `crates/clave-bar` and `crates/clave-types` trees
# (the bar and the only crate it links) must hash equal at that commit and at
# HEAD. That is a STRONGER claim than tag equality, not a weaker one — tag
# equality never looked at the source at all. Anything git cannot resolve
# (an unknown tag, a dirty tree, `dev`) stays a failure.
TAG_VERDICT="absent"
if [[ -n "$TAIL_MATCH" ]]; then
  TAG_VERDICT="present"
else
  # Which build DID load, per the newest loaded line we can see.
  LOADED_BUILD="$(printf '%s\n' "$LOADED_TAIL" | grep -o 'build=[0-9a-f]\{7,\}' | tail -1 | cut -d= -f2)"
  measure "loaded build tag (differs from HEAD ${HEAD_TAG})" "${LOADED_BUILD:-empty}"
  if [[ -n "$LOADED_BUILD" ]]; then
    SAME_SOURCE="yes"
    for crate in crates/clave-bar crates/clave-types; do
      A="$(git -C "$ROOT" rev-parse "${LOADED_BUILD}:${crate}" 2>/dev/null || echo unknown-a)"
      B="$(git -C "$ROOT" rev-parse "HEAD:${crate}" 2>/dev/null || echo unknown-b)"
      measure "${crate} tree: loaded=${A} head=${B}" "$([[ "$A" == "$B" ]] && echo same || echo DIFFERENT)"
      [[ "$A" == "$B" ]] || SAME_SOURCE="no"
    done
    # A dirty bar source means the running wasm cannot correspond to the
    # working tree whatever the trees say, so it is not an escape hatch.
    if [[ -n "$(git -C "$ROOT" status --porcelain -- crates/clave-bar crates/clave-types 2>/dev/null)" ]]; then
      SAME_SOURCE="no"
      note 'bar/types source is DIRTY — the loaded wasm cannot match the working tree, so the older tag is not accepted'
    fi
    if [[ "$SAME_SOURCE" == "yes" ]]; then
      TAG_VERDICT="present"
      # The running wasm is the source under test, so it is the tag every
      # later phase must grep for. Without this, phase 3's "a new tab's bar
      # LOADED" counts lines carrying a tag no bar ever printed, and reports
      # a product defect that is only a commit made mid-drive.
      BUILD_TAG="$LOADED_BUILD"
      note 'the loaded bar is build=%s, not HEAD (%s), but the bar and types trees are IDENTICAL at both — the running wasm is the source under test, and HEAD moved on host/docs only' "$LOADED_BUILD" "$HEAD_TAG"
    fi
  fi
fi
measure "loaded-tail matched line (verbatim)" "$TAIL_MATCH"
check "the loaded bar was built from the bar source under test" "$TAG_VERDICT" "present"
note 'same-HEAD re-runs cannot distinguish a stale load here — eyeball the tail timestamps'

# The clave that HOOKS will resolve must be able to read this sandbox's store.
#
# Added 2026-09-10 after a round lost to its absence. Hooks are registered in
# the shared ~/.claude/settings.json as bare `clave` — they must be, since that
# file is the maintainer's real one and a versioned path there is "the one
# leak" (#43/#44). But the sandbox's PATH shim does NOT reach the hook process
# Claude Code spawns: the pane's env survives (CLAVE_SESSION, CLAVE_STATE_DIR,
# ZELLIJ_PANE_ID are all correct, and `command -v clave` in the pane resolves
# to the shim), while its PATH does not. So bare `clave` lands on the RELEASE
# launcher, and the release binary is handed the SANDBOX store.
#
# That pairing is silent and total. An older clave hits an enum variant it has
# no name for, serde fails the whole struct, and every hook dies on the read —
# exiting 0 by Global Constraint with the message only on stderr. The fleet
# stops updating entirely and it presents as "the feature under test is
# broken": `card` did exactly this, and cost a round diagnosed as an adoption
# bug when adoption was fine.
#
# WHICH `clave`, though. This drive runs in its own terminal and the hook
# process does not inherit this shell's PATH, so the binary resolved HERE can
# differ from the one a hook lands on — and vouching for the wrong one is
# exactly the silent pass this check exists to prevent (#259 review). The one
# a SANDBOX hook lands on is knowable without guessing, though: hooks and the
# bar both invoke a BARE `clave` (`clave hook <Event>` in settings,
# `clave_binary "clave"` in config.kdl), and `dev launch` composes the
# instance's shim directory onto the FRONT of the PATH its panes inherit. So
# the shim is the hook's binary, and the assertion belongs there.
#
# Every other clave on some reachable PATH is still read, but as a HAZARD
# REPORT rather than a verdict on this build: a released binary older than
# this store's vocabulary cannot read it BY CONSTRUCTION — that is the
# one-way cost of adding a variant (clave-types `lenient`), and `just
# release` is what resolves it. It bites only if it wins a PATH race against
# the shim, which is the #43/#44 leak; named here with its reason so a real
# race reads as a race instead of as a mystery.
#
# `ls` is the cheapest command that reads the store and nothing else. Stderr,
# not exit code — `clave` reports a store read failure there and still exits 0.
store_read_err() {
  # The 2>&1 BEFORE >/dev/null is the point, not a slip: stderr takes the
  # current stdout and then stdout is dropped, so this returns the message
  # and never the listing. Empty output is the clean read.
  # shellcheck disable=SC2069
  CLAVE_STATE_DIR="$STATE_DIR" CLAVE_DATA_DIR="$DATA_DIR" "$1" ls 2>&1 >/dev/null || true
}

SHIM_CLAVE="$("$CLAVE_BIN" dev instance --field shim 2>/dev/null || true)/clave"
measure "the clave a sandbox hook resolves (the shim, first on the pane PATH)" "$SHIM_CLAVE"
if [[ -x "$SHIM_CLAVE" ]]; then
  measure "version of the shim clave" "$("$SHIM_CLAVE" --version 2>&1 || true)"
  SHIM_READ="$(store_read_err "$SHIM_CLAVE")"
  check "the shim clave reads this sandbox store" "${SHIM_READ:-clean}" "clean"
  note 'a failure on that line means EVERY hook in this sandbox no-ops silently — the drive below would read as broken features'
else
  check "the shim clave is executable (dev launch puts it first on PATH)" "missing" "present"
fi

DRIVE_CLAVE="$(command -v clave 2>/dev/null || true)"
LOGIN_CLAVE="$(env -i HOME="$HOME" bash -lc 'command -v clave' 2>/dev/null || true)"
measure "clave on this drive shell's PATH" "${DRIVE_CLAVE:-<none on PATH>}"
measure "clave on a clean login PATH" "${LOGIN_CLAVE:-<none on PATH>}"
OTHER_CLAVES=()
for CAND in "$DRIVE_CLAVE" "$LOGIN_CLAVE"; do
  [[ -n "$CAND" ]] || continue
  [[ "$CAND" == "$SHIM_CLAVE" ]] && continue
  # The `+()` guard is what keeps an empty array safe under `set -u`.
  [[ " ${OTHER_CLAVES[*]+${OTHER_CLAVES[*]}} " == *" $CAND "* ]] && continue
  OTHER_CLAVES+=("$CAND")
done
for CAND in "${OTHER_CLAVES[@]+"${OTHER_CLAVES[@]}"}"; do
  measure "version of $CAND (shadowed by the shim inside the sandbox)" "$("$CAND" --version 2>&1 || true)"
  CAND_READ="$(store_read_err "$CAND")"
  if [[ -z "$CAND_READ" ]]; then
    measure "$CAND reads this sandbox store" "clean"
  elif [[ "$CAND_READ" == *"unknown variant"* ]]; then
    # Older than the store's vocabulary. Expected on a branch that adds a
    # variant; a verdict on the release train, not on this build.
    note '%s predates this store vocabulary and rejects the WHOLE store (%s) — shadowed by the shim here, fatal to every hook if it ever wins the PATH race (#44); `just release` is the resolution' "$CAND" "$CAND_READ"
  else
    # Any other read failure is a real red: not a vocabulary gap, so it is
    # permissions, a corrupt store, or a binary that cannot run at all.
    check "$CAND reads this sandbox store" "$CAND_READ" "clean"
  fi
done

# config.kdl <-> layout.kdl identity pair (the #44 self-check `just sandbox`
# already runs at stage time — re-asserted here because config coherence can
# rot between staging and this run, e.g. a `clave setup` run by hand).
CFG="$DATA_DIR/config.kdl"
LAY="$DATA_DIR/layout.kdl"
LAUNCH="$DATA_DIR/launch.kdl"
# Existence AND content are separate failures — an extraction that comes
# back empty must FAIL on its own, never silently become an operand of the
# identity check below. Otherwise a `clave_binary` pattern that matches in
# NEITHER file leaves both sides "empty" and the comparison false-PASSES
# (same mechanism sandbox-setup.sh's own #44 self-check guards against,
# scripts/sandbox-setup.sh:212-213 — `grep -q 'clave_binary'` per file,
# failed closed, before any cross-file comparison is attempted).
for f in "$CFG" "$LAY"; do
  if [[ ! -f "$f" ]]; then
    check "present: $(basename "$f")" "missing" "present"
  elif ! grep -q 'clave_binary' "$f" 2>/dev/null; then
    check "carries clave_binary: $(basename "$f")" "missing" "present"
  fi
done
CFGVAL="$(grep -o 'clave_binary "[^"]*"' "$CFG" 2>/dev/null | sort -u | tr '\n' ',')"
LAYVAL="$(grep -o 'clave_binary "[^"]*"' "$LAY" 2>/dev/null | sort -u | tr '\n' ',')"
check "identity pair config.kdl<->layout.kdl" "$CFGVAL" "$LAYVAL"

# launch.kdl is asserted ONLY here, post-launch — the stale-by-design trap
# (RELEASE-RUNBOOK: `just release` never rewrites it, only a cold start
# does, so pre-launch it is either absent or a leftover from a PREVIOUS run
# and asserting on it then would fail a perfectly healthy sandbox).
if [[ -f "$LAUNCH" ]]; then
  if ! grep -q 'clave_binary' "$LAUNCH" 2>/dev/null; then
    check "carries clave_binary: $(basename "$LAUNCH")" "missing" "present"
  fi
  LAUNCHVAL="$(grep -o 'clave_binary "[^"]*"' "$LAUNCH" 2>/dev/null | sort -u | tr '\n' ',')"
  check "identity pair config.kdl<->launch.kdl (post-launch)" "$LAUNCHVAL" "$CFGVAL"
else
  check "present: launch.kdl (post-launch)" "missing" "present"
fi

# The ROW GEOMETRY is launch-baked (#232), so it is a preflight fact and not a
# runtime one: the layout carries both the bar's fixed pane width and the
# `row_height` plugin-config key, and a LIVE bar can neither resize its own
# pane nor swap its own config. The failure this catches is a STALE STAGE — a
# store asking for one geometry while the launched layout was generated by a
# binary that bakes another (or by one that had never heard of this mode at
# all, which is the skew that killed a whole session's hooks on 2026-09-10).
# The bar then renders one geometry into another's pane and nothing anywhere
# says so.
QA_REFUSED_BEFORE="$(count_push_refusals)"
measure "push refusals in the sandbox log before this run (forensic baseline)" "$QA_REFUSED_BEFORE"

P0_ROW_HEIGHT="$(jq -r '.store.row_height' <<<"$STATUS_JSON" 2>/dev/null)"
check_nonempty "store row_height readable" "$P0_ROW_HEIGHT"
# Every `row_height` in the launched layout, deduped: the bar pane node and
# every MessagePlugin keybind bake it, and zellij matches a pipe's
# destination on (location, configuration) EXACTLY — so two spellings in one
# file is two bars, not one (setup.rs:101's warning, asserted).
P0_LAY_RH="$(grep -o 'row_height "[^"]*"' "$LAUNCH" 2>/dev/null | sort -u | tr -d '\n')"
check "launch.kdl bakes ONE row_height, and it is the store's" \
  "$P0_LAY_RH" "row_height \"${P0_ROW_HEIGHT}\""
P0_CFG_RH="$(grep -o 'row_height "[^"]*"' "$CFG" 2>/dev/null | sort -u | tr -d '\n')"
check "config.kdl agrees with launch.kdl on row_height" "$P0_CFG_RH" "$P0_LAY_RH"

# The two declared bar widths. Both geometries live in the layout as
# `swap_tiled_layout` nodes, so the file carries exactly two distinct bar
# pane sizes — expanded and collapsed — and the collapsed one must be the
# smaller of the pair. Asserted as a PAIR rather than against a hardcoded
# number wherever possible, so this survives a future mode; the card's own
# numbers are then pinned from the lock (§1: 48 expanded, 16 collapsed),
# which is the independent expectation, not a re-derivation of the generator.
P0_BAR_SIZES="$(grep -o 'pane size=[0-9]* borderless=true' "$LAUNCH" 2>/dev/null |
  grep -o '[0-9]*' | sort -n -u | tr '\n' ' ' | sed 's/ $//')"
measure "bar pane widths declared in launch.kdl" "$P0_BAR_SIZES"
check "exactly two declared bar widths" "$(wc -w <<<"$P0_BAR_SIZES" | tr -d ' ')" "2"
if [[ "$P0_ROW_HEIGHT" == "card" ]]; then
  check "the card's ratified geometry (lock §1: 16 collapsed, 48 expanded)" \
    "$P0_BAR_SIZES" "16 48"
else
  note 'row_height=%s — the card pair is not asserted; widths recorded above' "$P0_ROW_HEIGHT"
fi

# Permission cache seeded under BOTH key forms (K7, #178-adjacent class: a
# partial match withholds EVERY pipe, not just the missing permission).
if [[ "$(uname)" == "Darwin" ]]; then
  PERM_CACHE="$HOME/Library/Caches/org.Zellij-Contributors.Zellij/permissions.kdl"
else
  PERM_CACHE="$HOME/.cache/zellij/permissions.kdl"
fi
WASM="$DATA_DIR/clave-bar.wasm"
if [[ -f "$PERM_CACHE" ]]; then
  PERM_COUNT="$(grep -c -F "$WASM" "$PERM_CACHE" 2>/dev/null || true)"
else
  PERM_COUNT=0
fi
check_min "permission cache carries both key forms ($WASM)" "${PERM_COUNT:-0}" 2

# Zero orphan `zellij pipe` processes (P7, #140 — an orphan hammers the
# router at full core; "empty" IS the healthy reading, print it as the word).
# Probed TWICE ~2s apart (#183 review): a healthy in-flight client from any
# session lives inside its ~1s window and matches at most one probe; only a
# pid present in BOTH probes has outlived the window and is the stale
# signature. Machine-wide pipe quiescence is NOT a precondition.
PIPES_A="$(pgrep -f 'zellij pipe' 2>/dev/null || true)"
sleep 2
PIPES_B="$(pgrep -f 'zellij pipe' 2>/dev/null || true)"
ORPHANS="$(comm -12 <(sort <<<"$PIPES_A") <(sort <<<"$PIPES_B") | xargs)"
if [[ -n "$ORPHANS" ]]; then
  measure "stale pipe forensics (pgrep -fl)" "$(pgrep -fl 'zellij pipe' 2>/dev/null || true)"
fi
check "orphan 'zellij pipe' processes (pid in both probes, 2s apart)" "$ORPHANS" ""

# ===========================================================================
# Phase 1 — baseline join
# ===========================================================================
phase "P1-baseline-join"

# The launch's width asks, RECORDED: a tab baked in the other mode asks once
# to correct itself (#89), so a launch is not necessarily silent. The flap's
# shape is many asks per instance for the width the pane already has; phase
# 4 asserts the zero, this line is the launch's own reading beside it.
measure "width asks since the launch mark (sandbox instances, one line per ask)" \
  "$(swap_ask_count_since "$LOGMARK") over $(sandbox_instance_count) instances"

STATUS_JSON="$(dev_status)"
measure "dev status (raw)" "$STATUS_JSON"

TOTAL_ROWS="$(jq '.store.agents | length' <<<"$STATUS_JSON" 2>/dev/null)"
DORMANT_COUNT="$(jq '[.store.agents[] | select(.tab_id == null)] | length' <<<"$STATUS_JSON" 2>/dev/null)"
measure "total rows" "$TOTAL_ROWS"
measure "dormant rows (tab_id null)" "$DORMANT_COUNT"

# SEEDED rows, counted apart from everything else in the store. A scenario's
# rows carry deterministic `c85c` uuids (dev.rs `scenario_uuid`); anything
# else is residue from an earlier drive, because this drive MINTS a row of its
# own at phase 2 rung 1 (`clave add`, by design, and its uuid is a real one).
#
# Counting the seeded rows rather than the whole store is what makes a re-run
# possible at all. The check used to be `total == 6`, which held only against
# a store staged seconds earlier — so the drive's own previous run turned
# phase 1 red, and the loop became kill, re-stage, ask for a relaunch, drive,
# for every iteration. The property never needed the total: it is about what
# the scenario put there and what the eager launch did with it.
SEEDED_ROWS="$(jq '[.store.agents | keys[] | select(startswith("00000000-0000-4000-8000-c85c"))] | length' <<<"$STATUS_JSON" 2>/dev/null)"
RESIDUE_ROWS=$((TOTAL_ROWS - SEEDED_ROWS))
measure "seeded rows (c85c uuids)" "$SEEDED_ROWS"
measure "non-seeded rows (this or an earlier drive's own creations)" "$RESIDUE_ROWS"
SEEDED_DORMANT="$(jq '[.store.agents | to_entries[] | select(.key | startswith("00000000-0000-4000-8000-c85c")) | select(.value.tab_id == null)] | length' <<<"$STATUS_JSON" 2>/dev/null)"
measure "dormant SEEDED rows (tab_id null)" "$SEEDED_DORMANT"

if [[ "$SCENARIO" == "qa-fleet" ]]; then
  # qa-fleet seeds 6 dormant rows; cold start's eager-launch selection
  # (setup.rs `eager_row` — the most-recent row whose cwd still exists)
  # auto-resumes exactly one of them into a live tab, so the STEADY STATE
  # this drive measures is 6 seeded / 5 still dormant / 1 bound.
  #
  # IS THIS STAGE FRESH. Asked first, and answered plainly, because the drive
  # is NOT idempotent and never can be: phases 2-5 bind dormant rows, mint a
  # row, churn tabs and toggle width — they consume the very starting shape
  # they assert against. A second run on the same stage therefore fails HERE,
  # on a count that is a perfectly correct reading of the previous run's
  # leftovers, and the red says nothing about the code under test. Naming
  # that cause costs two lines and saves the next agent the half hour this
  # cost: the counts below are only meaningful against a fresh stage.
  SEEDED_BOUND=$((SEEDED_ROWS - SEEDED_DORMANT))
  if [[ "$SEEDED_BOUND" -gt 1 || "$RESIDUE_ROWS" -gt 0 ]]; then
    cat >&2 <<EOF

  STALE STAGE: ${SEEDED_BOUND} seeded rows are bound (a fresh stage has
  exactly 1, the eager resume) and ${RESIDUE_ROWS} non-seeded row(s) are
  present (phase 2 rung 1 mints one per run, by design).

  This drive consumes its own starting conditions, so every count below is
  reading the PREVIOUS run rather than this build. Re-stage and drive in one
  command — it waits for your launch:

      zellij kill-session ${SESSION} && zellij delete-session --force ${SESSION}
      just qa ${SCENARIO}

  Continuing anyway, so the phases that do not depend on the starting shape
  still report; treat every count in phases 1-5 as suspect.

EOF
    note 'STALE STAGE — seeded_bound=%s (fresh: 1), residue=%s. Counts in phases 1-5 read the previous run. Re-stage with `just qa %s`.' "$SEEDED_BOUND" "$RESIDUE_ROWS" "$SCENARIO"
  fi
  check "the stage is fresh (seeded rows bound == 1, the eager resume)" "$SEEDED_BOUND" "1"
  check "seeded rows == scenario seed count" "$SEEDED_ROWS" "6"
  check "dormant seeded rows after eager resume" "$SEEDED_DORMANT" "5"
fi

# The eager-launch row's tab_id bound (the #178 resume face, P11). The most
# recent row by last_interacted is the eager_row() candidate; qa-fleet's
# only cwd-deleted row (`ghost`) is also its OLDEST, so max-recency alone
# picks the same row eager_row()'s cwd-liveness filter would.
EAGER_UUID="$(jq -r '.store.agents | to_entries | sort_by(-.value.last_interacted) | .[0].key // empty' <<<"$STATUS_JSON" 2>/dev/null)"
EAGER_TID="$(jq -r --arg u "$EAGER_UUID" '.store.agents[$u].tab_id // empty' <<<"$STATUS_JSON" 2>/dev/null)"
measure "eager-launch candidate uuid" "$EAGER_UUID"
check_nonempty "eager-launch row tab_id bound" "$EAGER_TID"

# Viewport geometry — measured via ct.sh dump, RECORDED, not asserted (host
# window size is not programmable; the `tall` scenario's job, not this one's).
PANES_JSON="$(ct_list_panes)"
PANES_RC=$?
check "ct.sh list-panes -t -c -j (join/viewport)" "$([[ $PANES_RC -eq 0 ]] && echo ok || echo failed)" "ok"
# Selectors are FLAT (final review BLOCKER 1): zellij's `PaneListEntry`
# declares `#[serde(flatten)] pane_info: PaneInfo` (zellij-utils 0.44.3
# data.rs ~2350) — the JSON `ct.sh list-panes` emits has no `pane_info` key;
# `is_plugin`/`plugin_url`/`pane_content_columns`/etc sit directly on the
# pane object.
# NOT bar geometry: list-panes omits the bar's panes entirely (FOOTGUNS,
# "list-panes does not show the clave-bar at all") — what this records is
# whatever plugin panes zellij DOES list (its own built-ins), kept as a
# viewport-shaped forensic. Bar width truth stays with the eyeball
# checkpoint; the `tall` scenario owns programmable geometry.
VIEWPORT="$(jq -c '[.[] | select(.is_plugin == true) | {tab_id, columns: .pane_content_columns, rows: .pane_content_rows}]' <<<"$PANES_JSON" 2>/dev/null)"
measure "plugin panes visible to list-panes (bar panes are NOT listed — geometry is the eyeball's)" "$VIEWPORT"

# Resume IDENTITY, not just bind (#183 review): the rotated row must resume
# its live_session — the second, rotated transcript — not its minted uuid
# (the pre-rotation conversation, the exact drift this scenario seeds to
# catch). The claude process's argv is read from pane_command; when no pane
# in the tab names claude that is UNKNOWN, not a mismatch (deepest-child
# trap, FOOTGUNS), so it demotes to a measure instead of failing.
#
# GATED on the scenario actually seeding a rotated row (2026-08-27 drive):
# only qa-fleet mints one, and every other scenario's seeded rows carry
# live_session=null by design (dev.rs #182) — hard-failing there measured
# the scenario, not the build. No rotation seeded → the whole identity
# block demotes to a measure, matching the usage text's qa-fleet-only note.
ROTATED_SEEDED="$(jq -r '[.store.agents[] | select(.live_session != null)] | length' <<<"$STATUS_JSON" 2>/dev/null)"
EAGER_LS="$(jq -r --arg u "$EAGER_UUID" '.store.agents[$u].live_session // empty' <<<"$STATUS_JSON" 2>/dev/null)"
if [[ "${ROTATED_SEEDED:-0}" -eq 0 ]]; then
  measure "eager row live_session" "scenario seeds no rotated row — resume-identity checks demoted to measure (qa-fleet owns them)"
else
check_nonempty "eager row live_session (rotated transcript seeded)" "$EAGER_LS"
EAGER_CMD="$(jq -r --argjson t "${EAGER_TID:-null}" '[.[] | select(.tab_id == $t and ((.pane_command // "") | test("claude")))] | .[0].pane_command // empty' <<<"$PANES_JSON" 2>/dev/null)"
if [[ -n "$EAGER_CMD" ]]; then
  check "eager resume targets live_session, not the minted uuid" \
    "$([[ "$EAGER_CMD" == *"--resume ${EAGER_LS}"* ]] && echo ok || echo "mismatch: $EAGER_CMD")" "ok"
else
  # Deepest-child fallback (#183 review round 2): the pane's command is a
  # child, so read the process table instead. A machine-wide ps scan is
  # user-global like zellij.log — but the NEEDLE here is a minted-per-drive
  # uuid, globally unique, so a match IS attributable: only this sandbox's
  # claude can carry `--resume <this uuid>` in its argv. A hit on the minted
  # uuid is a proven rotation miss (fail-closed); a hit on live_session is a
  # pass; neither resolvable stays a measure, unknown not mismatched.
  if pgrep -f -- "--resume ${EAGER_UUID}" >/dev/null 2>&1; then
    check "eager resume targets live_session, not the minted uuid (ps fallback)" \
      "mismatch: a process resumes the minted uuid ${EAGER_UUID}" "ok"
  elif [[ -n "${EAGER_LS}" ]] && pgrep -f -- "--resume ${EAGER_LS}" >/dev/null 2>&1; then
    check "eager resume targets live_session, not the minted uuid (ps fallback)" "ok" "ok"
  else
    measure "eager resume identity" "unresolvable — no pane_command in tab $EAGER_TID names claude and no process resumes either uuid (deepest-child, unknown not mismatched)"
  fi
fi
fi

# Store <-> layout join, unresolvables MARKED, never filtered (TESTING.md,
# "the join is not as easy as it looks" — `pane_command` is the pane's
# DEEPEST child process, so an agent pane routinely shows something other
# than `claude` and that is unknown, not a mismatch).
JOIN="$(jq -c --argjson panes "$PANES_JSON" '
  [ .store.agents | to_entries[] | select(.value.tab_id != null) |
    . as $e |
    ($panes | map(select(.tab_id == ($e.value.tab_id) and (.is_plugin | not)))) as $cands |
    { uuid: $e.key, tab_id: $e.value.tab_id,
      resolution:
        (if ($cands | length) == 0 then "UNRESOLVED no-terminal-pane-in-tab"
         elif ($cands | any(.pane_command // "" | test("claude"))) then "RESOLVED claude"
         else "UNRESOLVED " + ($cands[0].pane_command // "unknown")
         end) }
  ]' <<<"$STATUS_JSON" 2>/dev/null)"
measure "store<->layout join (bound rows)" "$JOIN"

SEQ="$(jq -r '.store.seq // empty' <<<"$STATUS_JSON" 2>/dev/null)"
measure "store seq" "$SEQ"

# ===========================================================================
# Phase 2 — bind ladder (mixed paths)
# ===========================================================================
phase "P2-bind-ladder"

count_dormant() {
  jq '[.store.agents[] | select(.tab_id == null)] | length' < <(dev_status) 2>/dev/null
}

# CAUTION — this count is USER-GLOBAL, not sandbox-scoped. The zellij log is
# one file for every session the user runs, and its 25-char source column
# truncates BOTH the stable wasm (~/.local/share/clave/…) and the sandbox
# wasm (~/.local/state/clave-dev-…/…) to the identical
# "/Users/<user>/.local"; plugin ids overlap across servers too. A dropped
# line is therefore unattributable to a session (first red run: rung 1
# measured a delta of 10, all of it the maintainer's live fleet). Twin
# deltas are RECORDED for forensics, never asserted — see FOOTGUNS.
count_eof_twins() {
  zlog_tail | grep -c -F 'clave-bar: dropped' 2>/dev/null || true
}

# uuid<TAB>cwd for every dormant row — the shared read behind the stale-row
# SKIP disclosure and the wakeable filter below.
dormant_rows() {
  jq -r '.store.agents | to_entries[] | select(.value.tab_id == null) | "\(.key)\t\(.value.cwd)"' <<<"$1" 2>/dev/null
}

dormant_uuids() {
  dormant_rows "$1" | cut -f1
}

# Dormant AND its cwd still exists on disk (final review BLOCKER 3): a
# scenario's cwd-deleted row (qa-fleet's `ghost`) takes `OpenDecision::Stale`
# the moment anything tries to open it (open.rs), and the bar's own commit
# gate then refuses it PERMANENTLY (model.rs, "STALE rows refuse the
# commit") — it can never leave the dormant set, so the ladder must never
# target it. Identified from the store, not the scenario name — honest and
# self-maintaining if the scenario ever changes.
wakeable_uuids() {
  dormant_rows "$1" | while IFS=$'\t' read -r u c; do
    [[ -n "$u" && -d "$c" ]] && printf '%s\n' "$u"
  done
}

log_stale_skips() {
  local status="$1" u c
  while IFS=$'\t' read -r u c; do
    [[ -z "$u" ]] && continue
    if [[ ! -d "$c" ]]; then
      printf '[%s %s] SKIP uuid=%s cwd=%s: cwd no longer exists on disk — the bar refuses this row'"'"'s commit by design (model.rs "STALE rows refuse the commit"), so the ladder never targets it; not #178\n' \
        "$CURRENT_PHASE" "$(ts)" "${u:0:13}" "$c"
    fi
  done < <(dormant_rows "$status")
}

# Seek-trace resting width — model BELIEF, not pane truth (the eyeball
# stays the oracle; QA-DRIVE.md phase-2: "seek-trace resting width ==
# target, labelled belief"). MAJOR 4: the old check compared a literal to
# itself ("recorded"=="recorded") and could never fail. The trace needs
# instrumentation NOT present in the shipped bar right now — seek-trace.sh's
# own header says the `CLAVE_DBG_seek` emitter was temporary and was
# removed, confirmed by grep: crates/clave-bar/src carries no such eprintln
# today. With nothing to compare, this is a single honest NOTE, never a
# tautological PASS. If the instrumentation is ever restored, this compares
# the seek's reported `cols=` against the model's real target (clave-types
# BAR_TARGET_COLS=54 / COLLAPSED_TARGET_COLS=30 — keep these two literals in
# sync with that file if they ever move).
width_belief() {
  local rung="$1" seek_line width_target seek_width
  if [[ -x "$SCRIPT_DIR/seek-trace.sh" ]]; then
    seek_line="$("$SCRIPT_DIR/seek-trace.sh" tail 1 2>/dev/null | tail -1)"
  fi
  if [[ -n "${seek_line:-}" ]]; then
    measure "rung $rung width belief (seek-trace)" "$seek_line"
    width_target=54
    [[ "$(jq -r '.store.collapsed // false' <<<"$(dev_status)" 2>/dev/null)" == "true" ]] && width_target=30
    seek_width="$(printf '%s' "$seek_line" | grep -oE 'cols=[0-9]+' | head -1 | cut -d= -f2)"
    check "rung $rung width belief" "$seek_width" "$width_target"
  else
    note 'rung %s width belief: measured=unavailable — seek-trace instrumentation is not in the shipped bar (scripts/seek-trace.sh header); not a check, not a PASS' "$rung"
  fi
}

BASE_STATUS="$(dev_status)"
log_stale_skips "$BASE_STATUS"

RUNG=0
ATTEMPTED=0
LANDED=0

# LIVE_NOW (live-block length, for the wake rungs' ROW arithmetic) starts
# from ONE real pane-list read and advances by the observed store delta
# thereafter — each CONFIRMED bind below (create or wake) is exactly one
# more live-block row — rather than re-deriving it via a row-count query
# every rung (BLOCKER 2 finding 2).
LIVE_START="$(count_live_tabs)"
LIVE_RC=$?
check "ct.sh list-panes -t -c -j (phase-2 baseline live-tab count)" \
  "$([[ $LIVE_RC -eq 0 && "$LIVE_START" =~ ^[0-9]+$ ]] && echo ok || echo failed)" "ok"

# ---------------------------------------------------------------------------
# Rung 1 — the scripted CREATE leg (MAJOR 5): `clave add`'s CLI path. The
# nav-commit legs below and `clave open` both only ever reach an EXISTING
# row (model.rs Effect::OpenAgent → the same `run_open` leg `clave open`
# exercises) — neither one MINTS a row. `clave add` is the only leg that
# does, which is what stories 9/21's "at least one scripted create" asked
# for.
#
# `add::run_add` drives real `fzf` for two picks (dir, then new/resume) and
# there is no TTY here, so `CLAVE_FZF_BIN` (discover.rs's sanctioned
# override — the same "override always wins" contract CLAVE_SESSION uses)
# points it at a stub that echoes stdin's FIRST line: the dir picker's
# first candidate is always the current directory (add.rs's own
# `zx[0] = cwd`), and the new/resume picker's first entry is "new" — both
# deterministic, no interaction needed. Real zoxide/git/zellij/claude still
# run underneath; only the interactive PICK is stubbed.
# ---------------------------------------------------------------------------
RUNG=$((RUNG + 1))
ATTEMPTED=$((ATTEMPTED + 1))
METHOD="scripted-create"

FZF_STUB="$QA_DIR/fzf-stub-$$.sh"
cat >"$FZF_STUB" <<'EOS'
#!/usr/bin/env bash
# Scripted stand-in for fzf (qa-drive.sh's scripted-create rung, MAJOR 5):
# always resolves the FIRST candidate on stdin — no interaction, no TTY.
head -n1
EOS
chmod +x "$FZF_STUB"

# `clave add` is session-hard since #183 (33e27fc): every internal zellij
# leg — dump-layout, pipe, new-tab — names `--session` explicitly and a
# dead named session exits 1. The env pinning below is the SECOND layer,
# the same belt-and-braces discipline ct.sh applies to every zellij touch
# (AGENTS.md: never even a read against the maintainer's session).
PRECREATE_LIVE="$(jq -r '.session_live // false' <<<"$(dev_status)" 2>/dev/null)"
check "session live immediately before scripted-create" "$PRECREATE_LIVE" "true"

TOTAL_BEFORE_CREATE="$(jq '.store.agents | length' <<<"$(dev_status)" 2>/dev/null)"
DORMANT_BEFORE_CREATE="$(count_dormant)"
UUIDS_BEFORE_CREATE="$(jq -r '.store.agents | keys[]' <<<"$(dev_status)" 2>/dev/null | sort)"
TWINS_BEFORE="$(count_eof_twins)"
LIVE_BEFORE="$(count_live_tabs)"
LIVE_RC=$?
check "ct.sh list-panes -t -c -j (rung $RUNG live-tab count)" "$([[ $LIVE_RC -eq 0 ]] && echo ok || echo failed)" "ok"
measure "rung $RUNG live tabs (= bar instances by design) before create" "$LIVE_BEFORE"

# Bound the call externally, since add.rs cannot bound itself (its
# dump-layout read and new-tab spawn both call `.output()`/`.status()` with
# no timeout, the same class of risk open.rs has): prefer coreutils
# `timeout`/`gtimeout` (same idiom as ct.sh); fall back to a bash-native
# watchdog when neither is on PATH.
CREATE_TIMEOUT="${CLAVE_OPEN_TIMEOUT:-30}"
# `clave add`'s stdout/stderr are inherited on purpose (tracing spec: never
# discarded) — zellij CLI chatter from its internal `zellij action` calls
# lands in this log between here and the result line below. The first red
# run's bare "1" was exactly that; this label keeps it attributable.
printf '[%s %s] rung %d (%s): clave add output follows (inherited, unprefixed)\n' \
  "$CURRENT_PHASE" "$(ts)" "$RUNG" "$METHOD"
if command -v timeout >/dev/null 2>&1; then
  ( cd "$ROOT" && unset ZELLIJ ZELLIJ_PANE_ID && \
    ZELLIJ_SESSION_NAME="$SESSION" CLAVE_SESSION="$SESSION" CLAVE_STATE_DIR="$STATE_DIR" CLAVE_DATA_DIR="$DATA_DIR" CLAVE_FZF_BIN="$FZF_STUB" \
    timeout "$CREATE_TIMEOUT" "$CLAVE_BIN" add )
  CREATE_RC=$?
elif command -v gtimeout >/dev/null 2>&1; then
  ( cd "$ROOT" && unset ZELLIJ ZELLIJ_PANE_ID && \
    ZELLIJ_SESSION_NAME="$SESSION" CLAVE_SESSION="$SESSION" CLAVE_STATE_DIR="$STATE_DIR" CLAVE_DATA_DIR="$DATA_DIR" CLAVE_FZF_BIN="$FZF_STUB" \
    gtimeout "$CREATE_TIMEOUT" "$CLAVE_BIN" add )
  CREATE_RC=$?
else
  ( cd "$ROOT" && unset ZELLIJ ZELLIJ_PANE_ID && \
    ZELLIJ_SESSION_NAME="$SESSION" CLAVE_SESSION="$SESSION" CLAVE_STATE_DIR="$STATE_DIR" CLAVE_DATA_DIR="$DATA_DIR" CLAVE_FZF_BIN="$FZF_STUB" \
    "$CLAVE_BIN" add ) &
  CREATE_PID=$!
  ( sleep "$CREATE_TIMEOUT"; kill -TERM "$CREATE_PID" 2>/dev/null ) &
  WATCHDOG_PID=$!
  wait "$CREATE_PID"
  CREATE_RC=$?
  kill "$WATCHDOG_PID" 2>/dev/null
  wait "$WATCHDOG_PID" 2>/dev/null
fi
rm -f "$FZF_STUB"

if ((CREATE_RC == 0)); then
  CREATE_RESULT="ok"
elif ((CREATE_RC == 124)) || ((CREATE_RC == 143)); then
  CREATE_RESULT="timeout=${CREATE_TIMEOUT}s"
else
  CREATE_RESULT="exit=${CREATE_RC}"
fi
measure "rung $RUNG ($METHOD) result" "$CREATE_RESULT"
if [[ "$CREATE_RESULT" == timeout=* ]]; then
  printf '\n[%s %s] WEDGE: clave add did not return within %ss — add.rs'"'"'s internal zellij/fzf calls are unbounded; this external timeout is the only bound this drive has.\n' \
    "$CURRENT_PHASE" "$(ts)" "$CREATE_TIMEOUT"
  fail_phase
fi

UUIDS_AFTER_CREATE="$(jq -r '.store.agents | keys[]' <<<"$(dev_status)" 2>/dev/null | sort)"
NEW_UUIDS="$(comm -13 <(printf '%s\n' "$UUIDS_BEFORE_CREATE") <(printf '%s\n' "$UUIDS_AFTER_CREATE"))"
NEW_COUNT="$(printf '%s\n' "$NEW_UUIDS" | grep -c .)"
check "rung $RUNG exactly one new store row minted" "$NEW_COUNT" "1"
CREATE_UUID="$(printf '%s\n' "$NEW_UUIDS" | head -n1)"
measure "rung $RUNG ($METHOD) minted uuid" "$CREATE_UUID"

TOTAL_AFTER_CREATE="$(jq '.store.agents | length' <<<"$(dev_status)" 2>/dev/null)"
check "rung $RUNG total rows incremented by exactly one" "$TOTAL_AFTER_CREATE" "$((TOTAL_BEFORE_CREATE + 1))"
# The minted row is EXCLUDED from this count: until its async bind lands
# (the 10s poll below), the fresh row has tab_id null and would transiently
# count as dormant — a sub-second race this check lost on the 2026-08-13
# third run (measured 6 where run two measured 5 at the same line). The
# check's actual claim is about the PRE-CREATE rows only: create mints
# fresh, it never wakes a seeded dormant row.
DORMANT_AFTER_CREATE="$(dormant_uuids "$(dev_status)" | grep -v -F "$CREATE_UUID" | grep -c .)"
check "rung $RUNG dormant count unchanged among pre-create rows (create mints fresh, never wakes dormant)" "$DORMANT_AFTER_CREATE" "$DORMANT_BEFORE_CREATE"

# Bounded wait (10s poll): the minted row's tab_id lands in store, the same
# async `clave bind` proxy every other rung waits on.
BOUND_TID=""
for _ in $(seq 1 10); do
  BOUND_TID="$(jq -r --arg u "$CREATE_UUID" '.store.agents[$u].tab_id // empty' < <(dev_status) 2>/dev/null)"
  [[ -n "$BOUND_TID" ]] && break
  sleep 1
done
check_nonempty "rung $RUNG tab_id bound uuid=${CREATE_UUID:0:13}" "$BOUND_TID"
[[ -n "$BOUND_TID" ]] && LANDED=$((LANDED + 1))

# `clave add` runs `zellij action new-tab`, not `zellij pipe` — no CliPipe
# broadcast, so sandbox-attributable twin traffic should be ~0. Recorded,
# not asserted: the count is user-global (see count_eof_twins).
TWINS_AFTER="$(count_eof_twins)"
TWIN_DELTA=$((TWINS_AFTER - TWINS_BEFORE))
measure "rung $RUNG EOF-twin delta (user-global log, unattributable; ~0 if sandbox-only)" "$TWIN_DELTA"

width_belief "$RUNG"

printf '[%s %s] BUDGET rung %d: attempted=%d landed=%d (the "2 then never again" signature is #178'"'"'s tell)\n' \
  "$CURRENT_PHASE" "$(ts)" "$RUNG" "$ATTEMPTED" "$LANDED"

# ---------------------------------------------------------------------------
# Remaining rungs — wake ladder, mixed paths continued (BLOCKER 2 + 3): nav
# pipes over every WAKEABLE dormant row, prediction-free. Target the first
# WAKEABLE row of the rendered dormant block. The earlier bottom-targeting
# was built on a false premise ("qa-fleet seeds commit_ord 0, ties sort
# uuid-desc, stale row at top"): the scenario actually seeds DISTINCT
# ordinals and the ghost row is deliberately the OLDEST (lowest ordinal),
# and model.rs `rows()` sorts each block by ordinal DESCENDING — so the
# ghost renders at the BOTTOM, exactly where the ladder aimed. The
# 2026-08-13 run proved it: row 7 picked, clave.log answered
# "cwd missing → stale" for the ghost uuid, nothing woke, red at rung 2 —
# BLOCKER 3's forgery in the flesh. Top of block = the HIGHEST-ordinal
# dormant row, which by that same seeding is always wakeable; the ghost
# can only reach the top once it is the last dormant row standing, and
# then `wakeable_uuids` is already empty and the loop has stopped before
# ever clicking it. Which uuid each click actually landed on is OBSERVED,
# never predicted (BLOCKER 2): snapshot the dormant set, wait, and read
# back whichever single uuid left it.
# ---------------------------------------------------------------------------
while :; do
  CUR_STATUS="$(dev_status)"
  WAKEABLE_NOW="$(wakeable_uuids "$CUR_STATUS")"
  [[ -z "$WAKEABLE_NOW" ]] && break
  RUNG=$((RUNG + 1))
  ATTEMPTED=$((ATTEMPTED + 1))
  METHOD="nav-pick-commit"

  DORMANT_BEFORE_SET="$(dormant_uuids "$CUR_STATUS")"
  DORMANT_BEFORE_COUNT="$(printf '%s\n' "$DORMANT_BEFORE_SET" | grep -c .)"
  LIVE_BEFORE="$(count_live_tabs)"
  LIVE_RC=$?
  check "ct.sh list-panes -t -c -j (rung $RUNG live-tab count)" "$([[ $LIVE_RC -eq 0 ]] && echo ok || echo failed)" "ok"
  measure "rung $RUNG live tabs (= bar instances by design) before pipes" "$LIVE_BEFORE"
  TWINS_BEFORE="$(count_eof_twins)"

  LIVE_NOW=$((LIVE_START + LANDED))
  # Rendered rank of the first WAKEABLE dormant row (#183 review): the block
  # head is not guaranteed wakeable in every scenario — ux-gate1 renders its
  # stale row ABOVE a wakeable one, so a bare head pick would no-op on it and
  # forge a bind regression. Dormant rows render by dormant_ord DESCENDING
  # (model.rs `dormant_ord`: commit_ord.max(carried), and carried is
  # NO_COMMITMENT=0 while tab_id is null — so for store-dormant rows the key
  # is commit_ord alone); walk that order and take the first uuid the
  # wakeable filter admits. In qa-fleet the ghost is the oldest and this
  # degenerates to the head, as before.
  TARGET_RANK=0
  while IFS= read -r u; do
    TARGET_RANK=$((TARGET_RANK + 1))
    printf '%s\n' "$WAKEABLE_NOW" | grep -qx -F "$u" && break
  done <<<"$(jq -r '.store.agents | to_entries | map(select(.value.tab_id == null)) | sort_by(-.value.commit_ord) | .[].key' <<<"$CUR_STATUS" 2>/dev/null)"
  ROW=$((LIVE_NOW + TARGET_RANK))
  # rc-gate both pipe legs (same discipline as ct_list_panes): a ct.sh
  # refusal here (session died mid-drive) would otherwise surface as "no
  # row left the dormant set" — a forged #178 signature. The refusal text
  # itself is already in this log via fd2; the check makes it gating.
  "$CT" pipe --name clave-nav -- "{\"row\":${ROW}}"
  PIPE1_RC=$?
  "$CT" pipe --name clave-nav -- '{"commit":true}'
  PIPE2_RC=$?
  check "rung $RUNG ct.sh pipe legs accepted (row+commit)" "$([[ $PIPE1_RC -eq 0 && $PIPE2_RC -eq 0 ]] && echo ok || echo failed)" "ok"
  measure "rung $RUNG ($METHOD) row picked (first wakeable rank in dormant block)" "$ROW"
  EXPECTED_TWINS=$((LIVE_BEFORE * 2))

  # Bounded wait (10s poll): which uuid left the dormant SET — not a
  # predicted one (BLOCKER 2).
  BOUND_UUID=""
  for _ in $(seq 1 10); do
    AFTER_SET="$(dormant_uuids "$(dev_status)")"
    BOUND_UUID="$(comm -23 <(printf '%s\n' "$DORMANT_BEFORE_SET" | sort) <(printf '%s\n' "$AFTER_SET" | sort))"
    [[ -n "$BOUND_UUID" ]] && break
    sleep 1
  done
  BOUND_COUNT="$(printf '%s\n' "$BOUND_UUID" | grep -c .)"
  check "rung $RUNG exactly one row left the dormant set" "$BOUND_COUNT" "1"
  BOUND_UUID="$(printf '%s\n' "$BOUND_UUID" | head -n1)"
  measure "rung $RUNG ($METHOD) observed uuid" "$BOUND_UUID"

  BOUND_TID="$(jq -r --arg u "$BOUND_UUID" '.store.agents[$u].tab_id // empty' < <(dev_status) 2>/dev/null)"
  check_nonempty "rung $RUNG tab_id bound uuid=${BOUND_UUID:0:13}" "$BOUND_TID"
  [[ -n "$BOUND_TID" ]] && LANDED=$((LANDED + 1))

  # Dormant count decremented on the next snapshot.
  DORMANT_AFTER="$(count_dormant)"
  check "rung $RUNG dormant count decremented" "$DORMANT_AFTER" "$((DORMANT_BEFORE_COUNT - 1))"

  # EOF-twin delta: would be pipes-sent x live-instances if the log were
  # sandbox-scoped. It is not (see count_eof_twins) — recorded, not asserted.
  TWINS_AFTER="$(count_eof_twins)"
  TWIN_DELTA=$((TWINS_AFTER - TWINS_BEFORE))
  measure "rung $RUNG EOF-twin delta (user-global log, unattributable; ~${EXPECTED_TWINS} if sandbox-only)" "$TWIN_DELTA"

  width_belief "$RUNG"

  printf '[%s %s] BUDGET rung %d: attempted=%d landed=%d (the "2 then never again" signature is #178'"'"'s tell)\n' \
    "$CURRENT_PHASE" "$(ts)" "$RUNG" "$ATTEMPTED" "$LANDED"
done

measure "bind ladder totals" "attempted=${ATTEMPTED} landed=${LANDED}"

# ===========================================================================
# Shared instruments for phases 3-4
# ===========================================================================
#
# Everything below is pane-INDEPENDENT on purpose (the drive's standing rule
# for churn and nav): tabs are named by their STABLE tab id, never by which
# pane happens to be focused inside them, and every zellij touch still goes
# through ct.sh. `TabInfo.tab_id` (zellij-utils data.rs:2269 "The stable
# identifier for this tab") is the same number the store's binds and
# `tab_order` are keyed on and the same one `list-panes` reports, so store
# truth and zellij truth join on it without a position anywhere in between.

# The sandbox's own event log (§6.9). UNLIKE the zellij log this one IS
# attributable: it lives in this instance's state dir, so a maintainer fleet
# cannot appear in it and a delta here is a real assertion, not a forensic.
EVLOG="$STATE_DIR/clave.log"

# Count lines for one `cmd` in the evlog. Absent file reads as 0 — the log is
# created on first write, and a sandbox that has logged nothing yet is a valid
# state, not an error.
evlog_count() {
  local n
  n="$(grep -c -F "\"cmd\":\"$1\"" "$EVLOG" 2>/dev/null)" || n=0
  printf '%s' "${n:-0}"
}

# The re-join, run after EVERY churn step (QA-DRIVE phase 3: "re-join after
# each"). Two store-side faces of the #55 mis-bind class:
#   - a bind pointing at a tab that is gone (the prune echo never landed), and
#   - two agents claiming one tab (the eviction that `bind-evict` logs).
# Bounded poll, because `clave prune-tabs` is fire-and-forget: the bar emits it
# on the close frame and the store lands it a beat later.
#
# FIRST LIVE RUN PENDING (3): that 15s is enough. If a run fails here, read the
# printed binds before assuming the window — a permanently stale bind and a
# slow echo look identical at second 15 and are not the same finding.
REJOIN_WAIT="${CLAVE_REJOIN_WAIT:-15}"
rejoin_check() {
  local label="$1" i status live stale dupes
  for i in $(seq 1 "$REJOIN_WAIT"); do
    status="$(dev_status)"
    if ! live="$(live_tab_ids)"; then
      check "$label ct.sh list-panes -t -c -j (re-join)" "failed" "ok"
      return
    fi
    stale="$(jq -r --argjson live "$live" \
      '[.store.agents | to_entries[] | select(.value.tab_id != null and ((.value.tab_id) as $t | $live | index($t) | not)) | "\(.key[0:13]):\(.value.tab_id)"] | join(",")' \
      <<<"$status" 2>/dev/null)"
    dupes="$(jq -r \
      '[.store.agents | to_entries[] | select(.value.tab_id != null) | .value.tab_id] | group_by(.) | map(select(length > 1) | .[0]) | join(",")' \
      <<<"$status" 2>/dev/null)"
    [[ -z "$stale" && -z "$dupes" ]] && break
    sleep 1
  done
  measure "$label live tab ids" "$live"
  measure "$label store binds (uuid:tab)" \
    "$(jq -r '[.store.agents | to_entries[] | select(.value.tab_id != null) | "\(.key[0:13]):\(.value.tab_id)"] | join(" ")' <<<"$status" 2>/dev/null)"
  check "$label no store bind points at a dead tab (waited ${REJOIN_WAIT}s)" "$stale" ""
  check "$label no two agents share one tab" "$dupes" ""
}

# The bar's LIVE block, top row first: ordinal DESC, ties by tab position ASC
# (model.rs `rows` sorts both blocks with one `rank_desc` comparator). Both
# inputs are readable from outside — `tab_order[tab]` is in the store snapshot
# and so is any bound agent's `commit_ord`, and `live_ord` takes the max of the
# two (model.rs `live_ord`).
predict_top_live_tab() {
  jq -r --argjson panes "$2" '
    . as $s
    | ($panes | map({tab_id, tab_position}) | unique_by(.tab_id)) as $tabs
    | [ $tabs[]
        | .tab_id as $t
        | { tab: $t,
            pos: .tab_position,
            ord: ([ ($s.store.tab_order[($t | tostring)] // 0),
                    ([$s.store.agents[] | select(.tab_id == $t) | .commit_ord] | max // 0) ] | max) } ]
    | sort_by([-.ord, .pos]) | .[0].tab // empty' <<<"$1" 2>/dev/null
}

# The bar's DORMANT block, top row first: the same comparator read from the
# other side — `dormant_ord` DESC, ties uuid DESC (model.rs `rows` sorts the
# dormant vector by `usize::MAX - i` over a uuid-ASCENDING list, which renders
# uuid-descending). For a store-dormant row `dormant_ord` is `commit_ord`
# alone: the carried leg reads `tab_order[a.tab_id]` and `tab_id` is null here.
#
# That last sentence is a DEPENDENCY, not a detail: a row still holding a bind
# to a dead tab is dormant to the bar and would shift every rank below it. The
# re-join above is what rules that out, which is why phase 4 runs after phase 3
# and not before it.
dormant_render_order() {
  jq -r '.store.agents | to_entries | map(select(.value.tab_id == null))
         | sort_by([.value.commit_ord, .key]) | reverse | .[].key' <<<"$1" 2>/dev/null
}

# Send one nav payload. Every CLI `zellij pipe` also delivers a blank EOF twin
# (FOOTGUNS) — the bar's empty-payload guard drops it, so one call here is one
# nav press, and the twin shows up only in the recorded twin delta.
nav_pipe() {
  "$CT" pipe --name clave-nav -- "$1"
}

# Point the executor election at a tab. `clave-visited` is the replicated
# beacon the bars broadcast themselves on every executed SwitchTab (model.rs
# AnnounceVisit/ConvergeVisit), and `nav_executor` answers that beacon ALONE
# (FOOTGUNS, #162) — so this pipe is indistinguishable from an organic one:
# every instance converges on it, and the bar standing in the named tab is
# the one elected. It matters because a NATIVE focus change emits no beacon:
# `focus_tab` moves zellij focus without moving the election, so a drive that
# only parks focus somewhere keeps talking to whichever bar the last beacon
# named. NOTE `beacon` also clears every instance's cursor, so an anchor must
# come BEFORE the pick it fronts, never after.
anchor_executor() {
  "$CT" pipe --name clave-visited -- "$1"
}

# Focus must NOT move. A dormant landing is a pure selection — `nav` returns
# `ArmPeek` at most and never a `SwitchTab` (model.rs, the `RowKey::Dormant`
# arm) — so ANY focus movement during a dormant walk means a second instance
# acted on the same press and walked its own LIVE ring. That is the shape
# stillness can see: an executor elected AFTER the pick, holding no dormant
# selection. It is NOT the whole single-executor story — two bars elected AT
# the pick both receive the broadcast, both select the same dormant row, both
# stay in-block, and focus never moves. That lockstep shape is counted
# instead, in the attributable evlog: each executor runs its own `clave open`
# at the commit (an already-live no-op still logs), so the open-count bracket
# around phase 4's walks+commit is the detector for it, and this check is
# only the detector for the post-pick shape.
assert_focus_unchanged() {
  local expected="$1" label="$2" got
  got="$(focused_tab_id)"
  check "$label focus unchanged (a dormant landing never switches tabs)" "${got:-empty}" "$expected"
}

# ===========================================================================
# Phase 3 — tab churn
# ===========================================================================
# Closes a NON-last tab, then the HIGHEST tab followed by a create, re-joining
# after each. Covers B14, B15 (#55), Z15, P4, P12/P13.
phase "P3-tab-churn"

P3_STATUS="$(dev_status)"
P3_PANES="$(ct_list_panes)"
P3_RC=$?
check "ct.sh list-panes -t -c -j (phase-3 baseline)" "$([[ $P3_RC -eq 0 ]] && echo ok || echo failed)" "ok"
P3_LIVE="$(jq -c '[.[] | .tab_id] | unique' <<<"$P3_PANES" 2>/dev/null)"
P3_LIVE_N="$(jq 'length' <<<"$P3_LIVE" 2>/dev/null)"
measure "phase-3 baseline live tabs" "${P3_LIVE} (count=${P3_LIVE_N})"

# Three tabs is the floor for the property, not a convenience: "a NON-last tab"
# and "the HIGHEST tab" have to be able to be different tabs, and closing the
# last tab standing closes the session.
if [[ ! "$P3_LIVE_N" =~ ^[0-9]+$ ]] || ((P3_LIVE_N < 3)); then
  printf '[%s %s] REFUSING: phase 3 needs at least 3 live tabs, measured %s. The bind ladder is what supplies them — read phase 2 above before reading this as a churn failure.\n' \
    "$CURRENT_PHASE" "$(ts)" "${P3_LIVE_N:-empty}"
  fail_phase
fi

EVICT_BEFORE="$(evlog_count bind-evict)"
measure "evlog bind-evict count before churn (sandbox-scoped, so this IS attributable)" "$EVICT_BEFORE"
TWINS_BEFORE="$(count_eof_twins)"

rejoin_check "phase-3 baseline:"

# ---------------------------------------------------------------------------
# Churn A — close a NON-last tab (B14: a close at a position above ours is the
# one whose prune had no retry before #55).
# ---------------------------------------------------------------------------
# The target: not the last position (that is the trivial close, and it is the
# one the old code got right), and not the highest id (that is churn B's job,
# and doing both to one tab would leave the recycle test with nothing to
# recycle). Preferring a BOUND tab is deliberate — an unbind is the half of the
# prune that touches an agent row.
A_TAB="$(jq -r --argjson status "$P3_STATUS" '
  ([.[] | {tab_id, tab_position}] | unique_by(.tab_id) | sort_by(.tab_position)) as $tabs
  | ($tabs | map(.tab_id) | max) as $maxid
  | [ $tabs[0:(($tabs | length) - 1)][] | .tab_id | select(. != $maxid) ] as $cands
  | ([$status.store.agents[] | .tab_id] | map(select(. != null))) as $bound
  | ([ $cands[] | select(. as $t | $bound | index($t)) ] + $cands)
  | .[0] // empty' <<<"$P3_PANES" 2>/dev/null)"
check_nonempty "churn A target: a non-last, non-highest tab exists" "$A_TAB"
A_UUID="$(jq -r --argjson t "${A_TAB:-null}" '.store.agents | to_entries[] | select(.value.tab_id == $t) | .key' <<<"$P3_STATUS" 2>/dev/null | head -n1)"
measure "churn A target tab" "tab=${A_TAB} bound_uuid=${A_UUID:0:13}"

focus_tab_checked "$A_TAB" "churn A:"
"$CT" close-tab
A_CLOSE_RC=$?
check "churn A ct.sh close-tab accepted" "$([[ $A_CLOSE_RC -eq 0 ]] && echo ok || echo failed)" "ok"

A_GONE="no"
for _ in $(seq 1 10); do
  if NOW_LIVE="$(live_tab_ids)"; then
    [[ "$(jq -r --argjson t "$A_TAB" 'index($t) // "gone"' <<<"$NOW_LIVE")" == "gone" ]] && A_GONE="yes" && break
  fi
  sleep 1
done
measure "churn A live tabs after close" "${NOW_LIVE:-empty}"
check "churn A closed tab left the live set" "$A_GONE" "yes"

rejoin_check "churn A:"
check "churn A no new bind-evict" "$(($(evlog_count bind-evict) - EVICT_BEFORE))" "0"
if [[ -n "$A_UUID" ]]; then
  measure "churn A closed tab's agent row" \
    "$(jq -r --arg u "$A_UUID" '"tab_id=" + ((.store.agents[$u].tab_id | tostring) // "null")' < <(dev_status) 2>/dev/null)"
fi

# Nav still answers, with exactly one focus change. The press is `{"row":1}` —
# the TOP of the live block — and the drive predicts which tab that is from the
# store's own ordering inputs, so a wrong landing is a real finding rather than
# an unreadable "focus moved somewhere".
#
# Focus is parked on some OTHER tab first, because a press that lands where
# focus already sits proves nothing. The parking move comes BEFORE the
# prediction, and that ordering is load-bearing: focusing a tab the store has
# never stamped fires a birth `clave touch`, which mints the highest ordinal
# going and makes THAT tab the top live row — predict first and the drive would
# be racing the ordering it is predicting. So: park, let the touch settle,
# predict, and if the parked tab is itself the top row, park somewhere else and
# try again rather than assert something vacuous.
nav_focus_check() {
  local label="$1" status panes want here got i samples rc cand tried
  panes="$(ct_list_panes)" || {
    check "$label ct.sh list-panes -t -c -j (nav check)" "failed" "ok"
    return
  }
  tried=""
  want=""
  for cand in $(jq -r '[.[] | .tab_id] | unique | .[]' <<<"$panes" 2>/dev/null); do
    focus_tab_checked "$cand" "$label parked:"
    sleep 2 # a birth `clave touch` is fire-and-forget; give it the beat
    status="$(dev_status)"
    panes="$(ct_list_panes)" || {
      check "$label ct.sh list-panes -t -c -j (nav check, post-park)" "failed" "ok"
      return
    }
    want="$(predict_top_live_tab "$status" "$panes")"
    tried="${tried}park=${cand}->top=${want:-empty} "
    [[ -n "$want" && "$want" != "$cand" ]] && break
  done
  measure "$label parking attempts (a park that is itself the top row proves nothing)" "$tried"
  here="$(focused_tab_id)"
  measure "$label predicted top live row (ordinal desc, ties by position)" "${want:-empty} (focus parked on ${here:-empty})"
  if [[ -z "$want" || "$want" == "$here" ]]; then
    note '%s nav focus check NOT EXERCISED: every tab tried is itself the top live row, so a press could not move focus. Not a pass.' "$label"
    return
  fi
  nav_pipe '{"row":1}'
  rc=$?
  check "$label nav pipe accepted" "$([[ $rc -eq 0 ]] && echo ok || echo failed)" "ok"
  for i in $(seq 1 10); do
    got="$(focused_tab_id)"
    [[ "$got" == "$want" ]] && break
    sleep 1
  done
  check "$label nav {\"row\":1} focused the predicted top live tab" "${got:-empty}" "$want"
  # …and then STOPS. A second executor answering the same press drags focus
  # somewhere else a beat later; three samples over ~3s is the bound this drive
  # can afford, and it is a bound, not a proof — say so rather than imply more.
  samples=""
  for i in 1 2 3; do
    sleep 1
    got="$(focused_tab_id)"
    # A failed read joins the sample set as the WORD, so it fails this check
    # rather than disappearing out of a de-duplicated list.
    samples="${samples}${got:-empty} "
  done
  measure "$label focus samples over the 3s after the press (one press, one landing)" "$samples"
  check "$label focus settled: no second landing within 3s" \
    "$(printf '%s' "$samples" | tr ' ' '\n' | grep -v '^$' | sort -u | paste -sd, -)" "$want"
}

nav_focus_check "churn A:"

# ---------------------------------------------------------------------------
# Churn B — close the HIGHEST tab, then create one. zellij RECYCLES tab ids
# (`get_new_tab_id` = last key + 1 over a BTreeMap, screen.rs:1617), so this is
# the sequence that hands a fresh tab a dead tab's id — B15/#55's ground.
# ---------------------------------------------------------------------------
B_STATUS="$(dev_status)"
B_PANES="$(ct_list_panes)"
B_RC=$?
check "ct.sh list-panes -t -c -j (churn B baseline)" "$([[ $B_RC -eq 0 ]] && echo ok || echo failed)" "ok"
B_TAB="$(jq -r '[.[] | .tab_id] | max // empty' <<<"$B_PANES" 2>/dev/null)"
check_nonempty "churn B target: the highest live tab id" "$B_TAB"
B_OLD_ORD="$(jq -r --arg t "${B_TAB}" '.store.tab_order[$t] // empty' <<<"$B_STATUS" 2>/dev/null)"
B_UUID="$(jq -r --argjson t "${B_TAB:-null}" '.store.agents | to_entries[] | select(.value.tab_id == $t) | .key' <<<"$B_STATUS" 2>/dev/null | head -n1)"
measure "churn B target tab" "tab=${B_TAB} tab_order_ordinal=${B_OLD_ORD:-empty} bound_uuid=${B_UUID:0:13}"

focus_tab_checked "$B_TAB" "churn B:"
"$CT" close-tab
B_CLOSE_RC=$?
check "churn B ct.sh close-tab accepted" "$([[ $B_CLOSE_RC -eq 0 ]] && echo ok || echo failed)" "ok"

B_GONE="no"
for _ in $(seq 1 10); do
  if NOW_LIVE="$(live_tab_ids)"; then
    [[ "$(jq -r --argjson t "$B_TAB" 'index($t) // "gone"' <<<"$NOW_LIVE")" == "gone" ]] && B_GONE="yes" && break
  fi
  sleep 1
done
measure "churn B live tabs after close" "${NOW_LIVE:-empty}"
check "churn B highest tab left the live set" "$B_GONE" "yes"
rejoin_check "churn B (post-close):"
check "churn B no new bind-evict (post-close)" "$(($(evlog_count bind-evict) - EVICT_BEFORE))" "0"

# The diff base for "what did the create add" is the POST-close set, not the
# phase baseline: recycling hands the new tab the closed tab's exact id, so a
# diff against the pre-close set is EMPTY on precisely the run that exercises
# the property, and the recycling measure and stamp assertions below would be
# unreachable. Read fresh, rc-gated — a refused read must not silently become
# the diff's operand.
B_LIVE_POSTCLOSE="$(live_tab_ids)"
B_POSTCLOSE_RC=$?
check "ct.sh list-panes -t -c -j (churn B post-close diff base)" \
  "$([[ $B_POSTCLOSE_RC -eq 0 ]] && echo ok || echo failed)" "ok"

# The create. `ct.sh new-tab` is the pane-independent create: no fzf, no agent,
# no CLI of ours — just zellij building a tab from the session's
# `default_tab_template`, which is what puts a bar in it. Two things then have
# to be true for the tab to be HOOKED UP rather than merely present: its bar
# loaded, and the store learned about the tab.
LOADED_BEFORE="$(bar_loaded_count)"
measure "clave-bar loaded lines carrying build=${BUILD_TAG} before create" "$LOADED_BEFORE"
"$CT" new-tab
NEWTAB_RC=$?
check "churn B ct.sh new-tab accepted" "$([[ $NEWTAB_RC -eq 0 ]] && echo ok || echo failed)" "ok"

NEW_IDS=""
for _ in $(seq 1 10); do
  if NOW_LIVE="$(live_tab_ids)"; then
    NEW_IDS="$(jq -r --argjson before "$B_LIVE_POSTCLOSE" '[.[] | select(. as $t | $before | index($t) | not)] | join(",")' <<<"$NOW_LIVE" 2>/dev/null)"
    [[ -n "$NEW_IDS" ]] && break
  fi
  sleep 1
done
measure "churn B live tabs after create" "${NOW_LIVE:-empty}"
check "churn B exactly one new tab id appeared" "$(printf '%s' "$NEW_IDS" | awk -F, 'NF{print NF}')" "1"
NEW_TAB="$NEW_IDS"

# The recycle. FIRST LIVE RUN PENDING (4): whether this session's server hands
# the closed id back. It is what screen.rs does, but it is a server detail and
# this drive does not get to assume it — so it is MEASURED, and the two stamp
# assertions below say plainly which of them the run actually exercised.
if [[ "$NEW_TAB" == "$B_TAB" ]]; then
  measure "churn B tab id recycling" "recycled: the new tab took the closed tab's id ${B_TAB}"
else
  note 'churn B: the new tab is id %s, not the closed %s — this run did NOT exercise recycling, so the inherited-stamp check below is a control, not the property.' "$NEW_TAB" "$B_TAB"
fi

# The birth stamp is fire-and-forget (`Effect::Touch` → `clave touch <tab>`),
# so it is polled for, not sampled once — and the poll doubles as the settle
# the nav check below needs, since a stamp landing mid-check would reorder the
# live block underneath it.
C_STATUS="$(dev_status)"
NEW_ORD=""
for _ in $(seq 1 10); do
  C_STATUS="$(dev_status)"
  NEW_ORD="$(jq -r --arg t "${NEW_TAB}" '.store.tab_order[$t] // empty' <<<"$C_STATUS" 2>/dev/null)"
  [[ -n "$NEW_ORD" && "$NEW_ORD" != "$B_OLD_ORD" ]] && break
  sleep 1
done
NEW_BOUND="$(jq -r --argjson t "${NEW_TAB:-null}" '[.store.agents | to_entries[] | select(.value.tab_id == $t) | .key[0:13]] | join(",")' <<<"$C_STATUS" 2>/dev/null)"
check "churn B the new tab inherited no bind (a dead agent must not follow its id)" "$NEW_BOUND" ""
measure "churn B new tab's tab_order ordinal" "${NEW_ORD:-empty} (closed tab's was ${B_OLD_ORD:-empty})"
if [[ -n "$B_OLD_ORD" ]]; then
  check "churn B the new tab carries no INHERITED stamp" \
    "$([[ "$NEW_ORD" == "$B_OLD_ORD" ]] && echo "inherited ${NEW_ORD}" || echo "not-inherited")" "not-inherited"
fi
# The other half of B15, deliberately NOT an assertion: `needs_birth_touch`
# latches per (instance, tab id) and zellij recycles ids, so a recycled tab can
# come back PERMANENTLY unstamped — which sorts it below every dormant row
# (FOOTGUNS, "birth_touched latches on the tab ID"). The tab born here carries
# a NEW bar instance with an empty latch, so the stamp is expected to land; a
# missing one is a live sighting of that class, and it belongs in the report as
# a finding, not as a red gate on a known-open defect.
if [[ -z "$NEW_ORD" ]]; then
  note 'churn B: the new tab has NO tab_order stamp. That is the birth_touch latch signature (B15/#55) — record it, do not read it as this phase failing.'
fi

LOADED_AFTER="$LOADED_BEFORE"
for _ in $(seq 1 10); do
  LOADED_AFTER="$(bar_loaded_count)"
  ((LOADED_AFTER > LOADED_BEFORE)) && break
  sleep 1
done
measure "clave-bar loaded lines carrying build=${BUILD_TAG} after create" "$LOADED_AFTER"
check_min "churn B the new tab's bar LOADED (fresh build-tagged loaded line; the bar is invisible to list-panes)" \
  "$((LOADED_AFTER - LOADED_BEFORE))" 1

rejoin_check "churn B (post-create):"
check "churn B no new bind-evict (post-create)" "$(($(evlog_count bind-evict) - EVICT_BEFORE))" "0"
nav_focus_check "churn B:"

# ---------------------------------------------------------------------------
# Churn C — a wake THROUGH the churn: the other half of "hooked up correctly".
# A `new-tab` proves a bar loads; only a wake proves a store row still binds
# after the tab set has been shuffled twice.
#
# Conditional on there being a dormant row to SPARE: phase 4 needs a dormant
# block of its own, and a drive that eats its own preconditions reports a
# refusal it caused. The skip is loud.
# ---------------------------------------------------------------------------
C_STATUS="$(dev_status)"
C_WAKEABLE="$(wakeable_uuids "$C_STATUS")"
C_WAKEABLE_N="$(printf '%s\n' "$C_WAKEABLE" | grep -c .)"
measure "churn C wakeable dormant rows" "$C_WAKEABLE_N"
if ((C_WAKEABLE_N >= 2)); then
  C_LIVE="$(live_tab_ids)"
  C_LIVE_RC=$?
  C_LIVE_N="$(jq 'length' <<<"$C_LIVE" 2>/dev/null)"
  # rc-gated because the live block's LENGTH is the offset every `{"row":N}`
  # pick is built on: a refused read would silently aim the pick a few rows off
  # and wake the wrong agent (phase 2's own discipline).
  check "churn C ct.sh list-panes -t -c -j (live block length)" \
    "$([[ $C_LIVE_RC -eq 0 && "$C_LIVE_N" =~ ^[0-9]+$ ]] && echo ok || echo failed)" "ok"
  C_TARGET=""
  C_RANK=0
  while IFS= read -r u; do
    C_RANK=$((C_RANK + 1))
    if printf '%s\n' "$C_WAKEABLE" | grep -qx -F "$u"; then
      C_TARGET="$u"
      break
    fi
  done < <(dormant_render_order "$C_STATUS")
  C_ROW=$((C_LIVE_N + C_RANK))
  C_DORMANT_BEFORE="$(dormant_uuids "$C_STATUS")"
  C_OPEN_BEFORE="$(evlog_count open)"
  measure "churn C wake target" "uuid=${C_TARGET:0:13} dormant_rank=${C_RANK} display_row=${C_ROW}"
  nav_pipe "{\"row\":${C_ROW}}"
  C_P1=$?
  nav_pipe '{"commit":true}'
  C_P2=$?
  check "churn C nav pipe legs accepted (row+commit)" "$([[ $C_P1 -eq 0 && $C_P2 -eq 0 ]] && echo ok || echo failed)" "ok"
  C_BOUND=""
  for _ in $(seq 1 15); do
    C_BOUND="$(comm -23 <(printf '%s\n' "$C_DORMANT_BEFORE" | sort) <(dormant_uuids "$(dev_status)" | sort))"
    [[ -n "$C_BOUND" ]] && break
    sleep 1
  done
  measure "churn C uuid that left the dormant set" "${C_BOUND:0:13}"
  check "churn C exactly one row left the dormant set" "$(printf '%s\n' "$C_BOUND" | grep -c .)" "1"
  check "churn C the row that woke is the one picked" "${C_BOUND}" "${C_TARGET}"
  C_TID="$(jq -r --arg u "${C_BOUND}" '.store.agents[$u].tab_id // empty' < <(dev_status) 2>/dev/null)"
  check_nonempty "churn C woken row bound to a tab after two closes and a create" "$C_TID"
  # One commit, one `clave open`. The evlog is sandbox-scoped, so this counts
  # EXECUTORS: two instances acting on one broadcast would log two.
  check "churn C exactly one clave open ran (one executor)" "$(($(evlog_count open) - C_OPEN_BEFORE))" "1"
  rejoin_check "churn C:"
else
  printf '[%s %s] SKIP churn C wake: %s wakeable dormant row(s), and phase 4 needs the dormant block. The re-join above still covers the churn; what is untested here is a wake THROUGH it.\n' \
    "$CURRENT_PHASE" "$(ts)" "$C_WAKEABLE_N"
fi

check "phase 3 no new bind-evict overall" "$(($(evlog_count bind-evict) - EVICT_BEFORE))" "0"
measure "phase 3 EOF-twin delta (user-global log, unattributable — forensic only)" \
  "$(($(count_eof_twins) - TWINS_BEFORE))"

# ===========================================================================
# Phase 4 — ring walk
# ===========================================================================
# Picks into the dormant block, walks both directions, wraps, and commits once.
# Covers P1 (#162), P2, P16, K8.
#
# What is being proved and HOW, because none of it is directly visible: the
# cursor is per-instance model state that no store and no zellij read exposes.
#   - single executor → two reads, because neither alone covers both #162
#     shapes: focus must not move during a dormant walk (a second executor
#     elected after the pick has no dormant selection, so its ring is the
#     LIVE one and it would switch tabs), and the attributable evlog `open`
#     count bracketing walks+commit must land at exactly one (two executors
#     elected at the pick walk the same block in lockstep and never move
#     focus — but each runs its own `clave open` at the commit);
#   - in-block + wrap → the walk is net-zero by construction (a full wrap, then
#     one step each way), so the COMMIT must land on the row that was picked.
#     The landing uuid is the walk's only witness, and predicting it is the
#     assertion;
#   - one commit, one tab → the evlog's `open` count and the live tab delta.
# The walk is driven twice from two different focused tabs: ring movement is
# supposed to work regardless of which tab you are standing in.
phase "P4-ring-walk"

P4_STATUS="$(dev_status)"
P4_LIVE="$(live_tab_ids)"
P4_RC=$?
check "ct.sh list-panes -t -c -j (phase-4 baseline)" "$([[ $P4_RC -eq 0 ]] && echo ok || echo failed)" "ok"
P4_LIVE_N="$(jq 'length' <<<"$P4_LIVE" 2>/dev/null)"
P4_DORMANT="$(dormant_render_order "$P4_STATUS")"
P4_DORMANT_N="$(printf '%s\n' "$P4_DORMANT" | grep -c .)"
P4_WAKEABLE="$(wakeable_uuids "$P4_STATUS")"
P4_WAKEABLE_N="$(printf '%s\n' "$P4_WAKEABLE" | grep -c .)"
measure "phase-4 blocks" "live=${P4_LIVE_N} dormant=${P4_DORMANT_N} wakeable=${P4_WAKEABLE_N}"
measure "phase-4 dormant block, rendered order (top first)" "$(printf '%s' "$P4_DORMANT" | tr '\n' ' ')"

# A ring of one cannot be walked and a block with nothing committable cannot be
# landed on. Both are refusals with the measured counts, never a soft pass.
if ((P4_DORMANT_N < 2)) || ((P4_WAKEABLE_N < 1)) || ((P4_LIVE_N < 2)); then
  printf '[%s %s] REFUSING: phase 4 needs >=2 dormant rows (>=1 wakeable) and >=2 live tabs; measured dormant=%s wakeable=%s live=%s.\n' \
    "$CURRENT_PHASE" "$(ts)" "$P4_DORMANT_N" "$P4_WAKEABLE_N" "$P4_LIVE_N"
  fail_phase
fi

P4_EVICT_BEFORE="$(evlog_count bind-evict)"
P4_TWINS_BEFORE="$(count_eof_twins)"

# The target and its display row. `{"row":N}` indexes the WHOLE rendered list
# (live block first), which is why the live block's length is the offset — the
# same arithmetic phase 2's ladder uses.
P4_TARGET=""
P4_RANK=0
while IFS= read -r u; do
  P4_RANK=$((P4_RANK + 1))
  if printf '%s\n' "$P4_WAKEABLE" | grep -qx -F "$u"; then
    P4_TARGET="$u"
    break
  fi
done <<<"$P4_DORMANT"
P4_ROW=$((P4_LIVE_N + P4_RANK))
check_nonempty "phase-4 target: the first WAKEABLE row of the dormant block" "$P4_TARGET"
measure "phase-4 target" "uuid=${P4_TARGET:0:13} dormant_rank=${P4_RANK} display_row=${P4_ROW}"

# Two tabs to stand in, chosen by position so they are as far apart in the tab
# bar as the fleet allows.
P4_PANES="$(ct_list_panes)"
P4_PANES_RC=$?
check "ct.sh list-panes -t -c -j (phase-4 tab positions)" "$([[ $P4_PANES_RC -eq 0 ]] && echo ok || echo failed)" "ok"
P4_FIRST_TAB="$(jq -r '[.[] | {tab_id, tab_position}] | unique_by(.tab_id) | sort_by(.tab_position) | .[0].tab_id // empty' <<<"$P4_PANES" 2>/dev/null)"
P4_LAST_TAB="$(jq -r '[.[] | {tab_id, tab_position}] | unique_by(.tab_id) | sort_by(.tab_position) | .[-1].tab_id // empty' <<<"$P4_PANES" 2>/dev/null)"
measure "phase-4 standing tabs" "first=${P4_FIRST_TAB} last=${P4_LAST_TAB}"
check "phase-4 the two standing tabs are different tabs" \
  "$([[ -n "$P4_FIRST_TAB" && "$P4_FIRST_TAB" != "$P4_LAST_TAB" ]] && echo ok || echo "first=${P4_FIRST_TAB} last=${P4_LAST_TAB}")" "ok"

# ---------------------------------------------------------------------------
# One walk leg: pick into the dormant block from the tab you are standing in,
# wrap the ring once, then step both ways. Every press is followed by a focus
# read, and that read is the assertion — see `assert_focus_unchanged`.
# ---------------------------------------------------------------------------
walk_leg() {
  local stand="$1" label="$2" i rc
  focus_tab_checked "$stand" "$label"
  # Elect the standing tab's bar before the pick. Without this the walk is
  # answered by whichever bar the LAST beacon named (a native focus change
  # emits none), so both walks would exercise one bar's cursor and the
  # second-instance coverage would be fake. The pipe is fire-and-forget and
  # the election is model state no outside read exposes — the commit landing
  # on the picked row is its witness.
  anchor_executor "$stand"
  rc=$?
  check "$label executor anchor pipe accepted (clave-visited ${stand})" "$([[ $rc -eq 0 ]] && echo ok || echo failed)" "ok"
  sleep 1
  nav_pipe "{\"row\":${P4_ROW}}"
  rc=$?
  check "$label pick pipe accepted (row ${P4_ROW})" "$([[ $rc -eq 0 ]] && echo ok || echo failed)" "ok"
  sleep 1
  assert_focus_unchanged "$stand" "$label after the pick,"
  # A FULL wrap: exactly as many `next` presses as the block has rows returns
  # the cursor to where it started (#112 — the walk wraps WITHIN one block and
  # never crosses into the live one).
  for i in $(seq 1 "$P4_DORMANT_N"); do
    nav_pipe '{"dir":"next"}'
    rc=$?
    check "$label next ${i}/${P4_DORMANT_N} pipe accepted" "$([[ $rc -eq 0 ]] && echo ok || echo failed)" "ok"
    sleep 1
    assert_focus_unchanged "$stand" "$label after next ${i}/${P4_DORMANT_N},"
  done
  # …and both directions: one step back, one step forward, net zero again.
  nav_pipe '{"dir":"prev"}'
  rc=$?
  check "$label prev pipe accepted" "$([[ $rc -eq 0 ]] && echo ok || echo failed)" "ok"
  sleep 1
  assert_focus_unchanged "$stand" "$label after prev,"
  nav_pipe '{"dir":"next"}'
  rc=$?
  check "$label closing next pipe accepted" "$([[ $rc -eq 0 ]] && echo ok || echo failed)" "ok"
  sleep 1
  assert_focus_unchanged "$stand" "$label after the closing next,"
}

# Walk 1 — standing in the FIRST tab. No commit: this leg exists to show the
# ring turning without spending the selection.
#
# The open-count bracket OPENS here, before any press: it is the attributable
# single-executor evidence (see `assert_focus_unchanged` — stillness cannot
# see two executors walking in lockstep, the evlog can).
P4_OPEN_BEFORE="$(evlog_count open)"
P4_SEQ_BEFORE="$(jq -r '.store.seq // empty' <<<"$P4_STATUS" 2>/dev/null)"
# The width seam. Every focus change resets a bar's walk budget, so a bar
# that misjudges its painted width asks for a swap on every tab the walk
# lands in — the devbox flap (2026-09-22, `pane_frames false`: the pane
# paints one column short and an exact comparison asked sixteen times in
# four seconds). No tab is toggled during a walk, so the ask count across
# both legs must be zero on any host, frames on or off.
P4_SWAP_MARK="$(zlog_now)"
walk_leg "$P4_FIRST_TAB" "walk 1 (standing in tab ${P4_FIRST_TAB}):"

# A walk is selection only: it writes nothing. Recorded rather than asserted —
# a live agent's hook can advance `seq` underneath any drive, and mistaking
# that for a nav write would be a false red.
measure "store seq across walk 1 (a walk selects; it should write nothing)" \
  "before=${P4_SEQ_BEFORE} after=$(jq -r '.store.seq // empty' < <(dev_status) 2>/dev/null)"

# ---------------------------------------------------------------------------
# Walk 2 — the same walk from the OTHER end of the tab bar, then the one
# commit. The leg's anchor re-elects THIS tab's bar (walk 1's beacon would
# otherwise keep answering — see `anchor_executor`), and that same anchor
# wipes every cursor, so re-picking is required, not redundant: the cursor is
# executor-local state, the bar in this tab builds its own from scratch, and
# that is the property being shown — the ring works from wherever you are
# standing, driven by whichever bar is standing there.
# ---------------------------------------------------------------------------
walk_leg "$P4_LAST_TAB" "walk 2 (standing in tab ${P4_LAST_TAB}):"

check "the two walks made no width ask (a bar at its painted width asks nothing; the frames-off flap asked on every focus change)" \
  "$(swap_ask_count_since "$P4_SWAP_MARK")" "0"

# The bracket's midpoint: a walk is selection only, so NO open may have run
# yet — and pinning zero here is what proves the ==1 after the commit came
# from the commit alone, not from a stray walk-time open cancelling against a
# commit that never landed.
check "the two walks ran no clave open (a walk selects; it opens nothing)" \
  "$(($(evlog_count open) - P4_OPEN_BEFORE))" "0"

P4_DORMANT_BEFORE="$(dormant_uuids "$(dev_status)")"
P4_LIVE_BEFORE_N="$(jq 'length' < <(live_tab_ids) 2>/dev/null)"
nav_pipe '{"commit":true}'
P4_COMMIT_RC=$?
check "commit pipe accepted (the Alt+Enter equivalent)" "$([[ $P4_COMMIT_RC -eq 0 ]] && echo ok || echo failed)" "ok"

P4_LANDED=""
for _ in $(seq 1 15); do
  P4_LANDED="$(comm -23 <(printf '%s\n' "$P4_DORMANT_BEFORE" | sort) <(dormant_uuids "$(dev_status)" | sort))"
  [[ -n "$P4_LANDED" ]] && break
  sleep 1
done
measure "commit: uuid that left the dormant set" "${P4_LANDED:0:13}"
check "commit woke exactly one row" "$(printf '%s\n' "$P4_LANDED" | grep -c .)" "1"
# THE ring assertion. Two full wraps and a step each way later, the selection
# must still be on the row that was picked — if the walk had left the block, or
# stepped by anything other than one, this lands somewhere else.
check "the walk stayed in-block and net-zero: the commit landed on the row picked" \
  "${P4_LANDED}" "${P4_TARGET}"
check "walks+commit ran exactly one clave open (one executor, never two — the lockstep detector)" \
  "$(($(evlog_count open) - P4_OPEN_BEFORE))" "1"
P4_LIVE_AFTER_N="$(jq 'length' < <(live_tab_ids) 2>/dev/null)"
check "commit opened exactly one tab" "$P4_LIVE_AFTER_N" "$((P4_LIVE_BEFORE_N + 1))"
P4_TID=""
for _ in $(seq 1 10); do
  P4_TID="$(jq -r --arg u "${P4_LANDED}" '.store.agents[$u].tab_id // empty' < <(dev_status) 2>/dev/null)"
  [[ -n "$P4_TID" ]] && break
  sleep 1
done
check_nonempty "the committed row bound to its tab" "$P4_TID"
measure "focused tab after the commit (recorded, not asserted: the tab is created by clave open, and which tab zellij leaves focused is its call)" \
  "$(focused_tab_id)"
check "phase 4 no new bind-evict" "$(($(evlog_count bind-evict) - P4_EVICT_BEFORE))" "0"
measure "phase 4 EOF-twin delta (user-global log, unattributable — forensic only)" \
  "$(($(count_eof_twins) - P4_TWINS_BEFORE))"

rejoin_check "phase 4 (post-commit):"

# ===========================================================================
# Phase 5 — collapse burst
# ===========================================================================
# 12 paced toggles, 5 rapid, then one more. Covers B6-B9 (the toggle family),
# B10/B11 (parity durability), P5 (pipe-delivered presses).
#
# What is being proved and HOW: a toggle's only outside-visible truth is the
# store's `collapsed` flag — `PersistCollapse` executes on exactly ONE writer
# per press (main.rs `toggle_collapsed`: the pending ledger books the write,
# run_effects gates execution), so the flag flipping within a bounded wait
# proves press delivery, single-writer execution, and store persistence in
# one read. Pane geometry is deliberately NOT asserted — every automated
# width probe is a known liar (QA-DRIVE, eyeball checkpoints); the post-run
# eyeball owns it. Three shapes:
#   - each PACED press lands its flip (12 individual bounded waits — a press
#     that stops answering fails AT its ordinal, which is the B6 regression's
#     exact signature: the budget that spent itself and never refilled);
#   - the RAPID burst nets to parity (5 presses launched concurrently,
#     asserted only at the settled end — header ledger (6): the queued
#     shape awaits its first live run);
#   - the bar still answers AFTER the burst (press 18) — the #137-class
#     detector: a storm brake that turned into a lifetime budget died at
#     exactly this press shape, 33 clean presses then silence.
# 18 presses total: even, so the phase leaves `collapsed` where it found it.
phase "P5-collapse-burst"

toggle_pipe() {
  "$CT" pipe --name clave-toggle -- "1"
}

# Bounded wait for the store's collapsed flag to read `want`. Prints the
# settled value either way; the caller checks it.
# NO `// empty` here: jq's `//` treats `false` itself as absent, so
# `.store.collapsed // empty` can never observe the expanded state — run 3's
# P5 failed its first false-ward press on exactly that (the store had
# flipped; the probe was blind to it). A bare path prints true/false/null.
wait_collapsed() {
  local want="$1" i got=""
  for i in $(seq 1 10); do
    got="$(jq -r '.store.collapsed' < <(dev_status) 2>/dev/null)"
    [[ "$got" == "$want" ]] && break
    sleep 1
  done
  printf '%s' "${got}"
}

P5_STATUS="$(dev_status)"
P5_COLLAPSED0="$(jq -r '.store.collapsed // false' <<<"$P5_STATUS" 2>/dev/null)"
P5_SEQ0="$(jq -r '.store.seq' <<<"$P5_STATUS" 2>/dev/null)"
check_numeric "phase-5 start store seq readable" "$P5_SEQ0"
P5_TWINS_BEFORE="$(count_eof_twins)"
measure "phase-5 start" "collapsed=${P5_COLLAPSED0} seq=${P5_SEQ0}"

# One writer must exist before the first press: anchor the election to the
# tab that is focused right now (phase 4's commit left focus wherever zellij
# put it — recorded there, irrelevant here, the anchor just has to agree
# with SOME live tab so exactly one bar executes the persist).
P5_STAND="$(focused_tab_id)"
check_nonempty "phase-5 standing tab (focus read)" "$P5_STAND"
anchor_executor "$P5_STAND"
P5_RC=$?
check "phase-5 executor anchor pipe accepted (clave-visited ${P5_STAND})" "$([[ $P5_RC -eq 0 ]] && echo ok || echo failed)" "ok"
sleep 1

P5_EXPECT="$P5_COLLAPSED0"
for i in $(seq 1 12); do
  if [[ "$P5_EXPECT" == "true" ]]; then P5_EXPECT="false"; else P5_EXPECT="true"; fi
  toggle_pipe
  P5_RC=$?
  check "paced press ${i}/12 pipe accepted" "$([[ $P5_RC -eq 0 ]] && echo ok || echo failed)" "ok"
  check "paced press ${i}/12 landed (store collapsed flipped)" "$(wait_collapsed "$P5_EXPECT")" "$P5_EXPECT"
  sleep 1
done

# Writes per press <= 2 (QA-DRIVE spine): the persist is one store write and
# at most one companion snapshot push. Asserted over the paced 12 in
# aggregate — the sandbox fleet is hook-quiet by seed, and if it is not,
# FIRST LIVE RUN PENDING (7) says how this reads.
P5_SEQ_PACED="$(jq -r '.store.seq' < <(dev_status) 2>/dev/null)"
check_numeric "paced-12 store seq readable" "$P5_SEQ_PACED"
measure "store seq across the paced 12" "before=${P5_SEQ0} after=${P5_SEQ_PACED} delta=$((P5_SEQ_PACED - P5_SEQ0))"
check "paced writes per press <= 2 (12 presses, delta <= 24)" \
  "$(((P5_SEQ_PACED - P5_SEQ0) <= 24 ? 1 : 0))" "1"

# The rapid burst: five presses launched TOGETHER, judged only at the
# settled end. The CLI pipe BLOCKS until the plugin unblocks it, so a
# serial loop is five request-response round trips — no queue ever forms
# (CodeRabbit, PR #202). Backgrounding makes the burst real; each pipe's
# own exit status is still asserted once all five have finished. Order
# inside the burst is the queue's (header ledger (6)) and does not matter:
# five identical toggles net to parity regardless of arrival order.
P5_PIDS=()
for i in $(seq 1 5); do
  toggle_pipe &
  P5_PIDS+=("$!")
done
for i in "${!P5_PIDS[@]}"; do
  P5_RC=0
  wait "${P5_PIDS[$i]}" || P5_RC=$?
  check "rapid press $((i + 1))/5 pipe accepted" "$([[ $P5_RC -eq 0 ]] && echo ok || echo failed)" "ok"
done
# 12 + 5 = 17 presses: odd, so the settled flag must be the START's inverse.
if [[ "$P5_COLLAPSED0" == "true" ]]; then P5_EXPECT="false"; else P5_EXPECT="true"; fi
check "rapid burst settled at parity (17 presses = start inverted)" "$(wait_collapsed "$P5_EXPECT")" "$P5_EXPECT"

# Press 18 — after the burst. The press that found #137's corpse.
toggle_pipe
P5_RC=$?
check "post-burst press pipe accepted" "$([[ $P5_RC -eq 0 ]] && echo ok || echo failed)" "ok"
check "the bar still answers after the burst (press 18 landed, back to start)" \
  "$(wait_collapsed "$P5_COLLAPSED0")" "$P5_COLLAPSED0"

P5_SEQ_END="$(jq -r '.store.seq' < <(dev_status) 2>/dev/null)"
check_numeric "burst-end store seq readable" "$P5_SEQ_END"
measure "store seq across all 18 presses" "before=${P5_SEQ0} after=${P5_SEQ_END} delta=$((P5_SEQ_END - P5_SEQ0))"
check "total writes per press <= 2 (18 presses, delta <= 36)" \
  "$(((P5_SEQ_END - P5_SEQ0) <= 36 ? 1 : 0))" "1"
measure "phase 5 EOF-twin delta (user-global log, unattributable — forensic only)" \
  "$(($(count_eof_twins) - P5_TWINS_BEFORE))"

# --- the width the fleet actually rests at ---------------------------------
# The bug this branch found and fixed rests the bar at the WRONG WIDTH after
# a toggle — a walk ask spent on a pre-swap echo, and the walk needs all
# three. Nothing above can see it: the store's `collapsed` flag is the
# INTENT, and every press asserted so far only proves the intent landed.
#
# So: read the width from zellij and treat the reading as suspect until it
# proves itself. `list-panes` omits the bar entirely (FOOTGUNS), and the lock
# calls every automated width probe a known liar, which is why the phase-5
# eyeball exists. The layout dump does carry the bar's pane node — but a file
# that only ever echoes the DECLARED layout would make the assertion a
# tautology. The escape: measure in BOTH settled modes and assert only if the
# two readings DIFFER. A number that moves with the toggle is reporting the
# applied swap position, and nothing else can be.
dump_bar_widths() {
  local out
  out="$("$CT" dump-layout 2>/dev/null)" || return 1
  # The bar's own pane node: the size sits on the `pane` line that opens the
  # node whose plugin is the bar wasm, a few lines above it.
  grep -B4 -F 'clave-bar.wasm' <<<"$out" |
    grep -o 'size=[0-9]*' | grep -o '[0-9]*' | sort -n -u | tr '\n' ' ' | sed 's/ $//'
}

toggle_pipe
P5_RC=$?
check "width-probe press (to collapsed) accepted" "$([[ $P5_RC -eq 0 ]] && echo ok || echo failed)" "ok"
if [[ "$P5_COLLAPSED0" == "true" ]]; then P5_EXPECT="false"; else P5_EXPECT="true"; fi
check "width-probe press landed" "$(wait_collapsed "$P5_EXPECT")" "$P5_EXPECT"
sleep 2 # past the switch's own deafness (WIDTH_COOLDOWN_SECS) and its echoes
P5_W_A="$(dump_bar_widths || true)"
measure "bar pane width(s) in the dump, collapsed=${P5_EXPECT}" "$P5_W_A"

toggle_pipe
P5_RC=$?
check "width-probe press (back) accepted" "$([[ $P5_RC -eq 0 ]] && echo ok || echo failed)" "ok"
check "width-probe press landed back" "$(wait_collapsed "$P5_COLLAPSED0")" "$P5_COLLAPSED0"
sleep 2
P5_W_B="$(dump_bar_widths || true)"
measure "bar pane width(s) in the dump, collapsed=${P5_COLLAPSED0}" "$P5_W_B"

if [[ -z "$P5_W_A" || -z "$P5_W_B" ]]; then
  note 'the layout dump carries no bar pane size — width truth stays with the phase-5 eyeball'
elif [[ "$P5_W_A" == "$P5_W_B" ]]; then
  note 'the dump reports the same width in both modes (%s) — it is the DECLARED layout, not pane truth; asserting on it would be a tautology, so width truth stays with the phase-5 eyeball' "$P5_W_A"
else
  # It moves with the toggle, so it is the applied position. Two assertions
  # follow, and the second is the phase-5 eyeball's own question: one width
  # per mode across the WHOLE fleet — every instance flips itself off the
  # same store flag, so a second value is an instance resting wrong.
  check "one bar width across the fleet, collapsed=${P5_EXPECT} (no outliers)" \
    "$(wc -w <<<"$P5_W_A" | tr -d ' ')" "1"
  check "one bar width across the fleet, collapsed=${P5_COLLAPSED0} (no outliers)" \
    "$(wc -w <<<"$P5_W_B" | tr -d ' ')" "1"
  if [[ "$P0_ROW_HEIGHT" == "card" ]]; then
    # The card's ratified targets (lock §1), joined to the mode each belongs
    # to: this is the assertion the band-collision bug would have failed.
    if [[ "$P5_EXPECT" == "true" ]]; then
      check "the collapsed fleet rests at the card's 16" "$P5_W_A" "16"
      check "the expanded fleet rests at the card's 48" "$P5_W_B" "48"
    else
      check "the expanded fleet rests at the card's 48" "$P5_W_A" "48"
      check "the collapsed fleet rests at the card's 16" "$P5_W_B" "16"
    fi
  fi
fi

# ===========================================================================
# Phase 5b — the card's new cells, from the hook side
# ===========================================================================
# The four-line card added three cells that no earlier phase can reach:
# `wants` (what a blocked agent is asking for, quoted from its own
# notification), the subagent mark (a ledger over the transcript tail — every
# launch it holds, minus every one it has seen finish) and the status the two
# are gated on. Every one of them is written by `clave hook` into the store —
# so this phase drives the REAL binary against the REAL store file and the
# REAL transcript reader, which is the seam the unit tests cannot see. Three
# of this branch's defects lived exactly here: the mark read the
# statusLine-suppressed tail and so never landed in a released install; it
# read a count written after the Stop hook and so always a turn late; and a
# launch nothing closed held it for ever.
#
# Ordering: after the burst and before quiescence. It bumps `seq` and moves
# statuses, which no later phase reads, and it deliberately leaves the row
# IDLE — a row left Working would spin the card's 0.2s tick straight into
# phase 6's idle window.
phase "P5b-card-cells"

CLAUDE_PROJECTS="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects"

# The row to drive: scenario-seeded, not stale, and its own session id (so
# `resolve_transcript`'s derived path and the payload path agree). `stale`
# rows have no cwd left, and the rotated row's session id is not its uuid.
P5B_STATUS="$(dev_status)"
P5B_UUID="$(jq -r '
  .store.agents | to_entries
  | map(select(.value.stale != true))
  | map(select(.key | startswith("00000000-0000-4000-8000-c85c")))
  | .[0].key // empty' <<<"$P5B_STATUS" 2>/dev/null)"
check_nonempty "phase-5b target row (scenario-seeded, not stale)" "$P5B_UUID"

# Its transcript, found by globbing rather than by munging the cwd here — the
# munge is the binary's job and a second implementation of it in bash is a
# second thing to get wrong.
P5B_TAIL=""
for f in "$CLAUDE_PROJECTS"/*/"$P5B_UUID".jsonl; do
  [[ -f "$f" ]] && P5B_TAIL="$f" && break
done
check_nonempty "phase-5b transcript on disk (${P5B_UUID})" "$P5B_TAIL"
# GUARD, load-bearing: this phase APPENDS a line, and the only files it may
# ever append to are scenario fixtures — deterministic `c85c` uuids that
# `clave dev reset` sweeps (dev.rs `is_scenario_jsonl`). A real conversation
# must be unreachable from here even if the row selection above is changed.
check "phase-5b transcript is a scenario fixture (never a real conversation)" \
  "$(basename "$P5B_TAIL" | grep -c '^00000000-0000-4000-8000-c85c.*\.jsonl$' || true)" "1"

# One hook event, exactly as Claude Code fires it: the event name as argv,
# the payload on stdin, the sandbox's dirs in the environment. Exit status is
# never the signal — the hook exits 0 by Global Constraint — so every
# assertion below reads the STORE.
# THROUGH ct.sh, never `clave hook` directly. Setting CLAVE_STATE_DIR alone
# looks completely correct and is not: the store write lands in the sandbox,
# but `clave hook` also PUSHES, and it aims that push with `--session
# "$ZELLIJ_SESSION_NAME"` (hook.rs `own_session`) — which in a drive shell
# running inside the maintainer's fleet names HIS session. The push then
# arrives, precisely and by design, at the wrong bar. FOOTGUNS #281 says this
# in as many words and names this wrapper as the only sanctioned way to drive
# a hook; a hand-rolled `hook_fire` that bypassed it hung the maintainer's
# live session on 2026-09-11, which is why the rule is now enforced at the
# only site in this script that fires one.
hook_fire() {
  local event="$1" message="${2:-}" payload
  payload="$(jq -nc --arg s "$P5B_UUID" --arg t "$P5B_TAIL" --arg m "$message" \
    '{session_id:$s, transcript_path:$t} + (if $m == "" then {} else {message:$m} end)')"
  "$CT" --hook "$event" "$payload" 2>&1 | sed "s/^/[hook $event] /"
}

# One field of the driven row, after a bounded wait for it to read `want`.
# Prints the settled value either way; the caller checks it. A bare jq path,
# never `// empty` — `false` and `null` are both answers here and `//` cannot
# tell them from absent (run 3's P5 lost a press to exactly that).
wait_field() {
  local field="$1" want="$2" i got=""
  for i in $(seq 1 10); do
    got="$(jq -r --arg u "$P5B_UUID" ".store.agents[\$u].${field}" < <(dev_status) 2>/dev/null)"
    [[ "$got" == "$want" ]] && break
    sleep 1
  done
  printf '%s' "$got"
}

# --- the PR cache, warmed before anything is counted ------------------------
# `seq` is the only witness of a store write, and it is NOT a clean per-event
# counter. A hook whose row has a stale PR cache spawns `clave pr-sync`
# OUTSIDE the flock (pr.rs, deliberately — inside it would deadlock), and that
# second process writes on its own schedule, so its bump lands in whichever
# sampling window it pleases. A seeded row starts at `pr_checked: 0`, which
# `pr_is_stale` calls stale, so the ladder's first event always paid for one:
# the first live run of this phase measured 7 bumps for 5 events and read as a
# double-write that was never there.
#
# The confound is removed rather than budgeted for. One benign event first
# (`SessionEnd` — the state the ladder starts from anyway), then wait for the
# cache to reach the shape `pr_is_stale` calls fresh. `PR_TTL_SECS` is 300s
# and the ladder is seconds long, so nothing re-spawns inside it.
hook_fire SessionEnd >/dev/null
P5B_PR_WARM="stale"
for _ in $(seq 1 15); do
  if [[ "$(jq -r --arg u "$P5B_UUID" \
    '.store.agents[$u] | (.pr_checked > 0 and .pr_branch == .branch)' \
    < <(dev_status) 2>/dev/null)" == "true" ]]; then
    P5B_PR_WARM="fresh"
    break
  fi
  sleep 1
done
measure "phase-5b PR cache before the count begins" "$P5B_PR_WARM"
sleep 2 # the settling write is async — let it land before the baseline read

# Re-read: the baseline must be taken AFTER the warm-up, not from the status
# captured at phase start.
P5B_SEQ0="$(jq -r '.store.seq' < <(dev_status) 2>/dev/null)"
check_numeric "phase-5b start store seq readable" "$P5B_SEQ0"
measure "phase-5b row" "$P5B_UUID tail=$P5B_TAIL"

# --- wants: the words come off the agent's own notification -----------------
# The anchor is Claude Code's wording and is matched as a substring, because
# the CLI owns the sentence and has reworded it before.
hook_fire Notification "Claude needs your permission to use Bash"
check "a permission notification turns the row red" "$(wait_field status needs_you)" "needs_you"
check "and the card learns what it is asking for" "$(wait_field wants Bash)" "Bash"

# --- and it does not outlive the asking -------------------------------------
# `wants` is coextensive with NeedsYou on the wire; the next prompt is what
# ends the ask, and the words must go with it or the card contradicts its own
# glyph.
hook_fire UserPromptSubmit
check "the next turn clears the ask" "$(wait_field status working)" "working"
check "and takes its words with it" "$(wait_field wants null)" "null"

# --- the subagent mark: a ledger over the tail ------------------------------
# A fan-out opens on an `Agent` tool_use and closes on the task-notification
# naming the same `tool-use-id`. Both lines are appended to the real scenario
# transcript rather than simulated, so the real tail reader is what answers.
#
# This leg replaced one that appended `pendingBackgroundAgentCount` on a
# `turn_duration` line. That record is written AFTER the Stop hook, so a Stop
# could never see the turn it had just ended and the glyph outlived its agents
# by half an hour in the field (FOOTGUNS, and lock §4.6 has the numbers).
#
# The launch is stamped at the REAL current time, because the reader ages it:
# a launch past its bound is read as closed. `date -u -r` is BSD and `-d @` is
# GNU, so both are tried.
iso_utc_at() { # epoch seconds -> 2026-09-14T15:17:24.000Z
  date -u -r "$1" +%Y-%m-%dT%H:%M:%S.000Z 2>/dev/null ||
    date -u -d "@$1" +%Y-%m-%dT%H:%M:%S.000Z
}
P5B_NOW="$(date +%s)"
agent_launch_line() { # id, iso timestamp
  printf '{"type":"assistant","isSidechain":false,"timestamp":"%s","message":{"role":"assistant","content":[{"type":"tool_use","id":"%s","name":"Agent","input":{"description":"QA lane"}}]}}\n' "$2" "$1"
}
check_nonempty "phase-5b clock readable (the mark ages its launches)" \
  "$(iso_utc_at "$P5B_NOW")"
agent_launch_line toolu_qa5b "$(iso_utc_at "$P5B_NOW")" >>"$P5B_TAIL"
hook_fire Stop
check "a fan-out raises the subagent mark" "$(wait_field subagents true)" "true"
check "and the turn is over" "$(wait_field status "done")" "done"

# The same record carries background commands, and its `tool-use-id` then
# names a Bash call. The mark must match on the id, or every background
# command finishing would blank a mark its agents still earn.
printf '%s\n' '{"type":"queue-operation","operation":"enqueue","content":"<task-notification>\n<task-id>qa1</task-id>\n<tool-use-id>toolu_qabash</tool-use-id>\n<status>completed</status>\n<summary>Background command \"sleep 1\" completed (exit code 0)</summary>\n</task-notification>"}' >>"$P5B_TAIL"
hook_fire Stop
check "a background command finishing does not clear the mark" "$(wait_field subagents true)" "true"

# And the agent's own notification does close it — within seconds of the agent
# stopping, which is the whole reason the ledger replaced the turn-close count.
printf '%s\n' '{"type":"queue-operation","operation":"enqueue","content":"<task-notification>\n<task-id>qa2</task-id>\n<tool-use-id>toolu_qa5b</tool-use-id>\n<status>completed</status>\n<summary>Agent \"QA lane\" finished</summary>\n</task-notification>"}' >>"$P5B_TAIL"
hook_fire Stop
check "the agent's own notification clears the mark" "$(wait_field subagents false)" "false"

# --- a launch nothing ever closes must still stop holding the mark ----------
# A session killed with `kill -9` writes no notification and fires no
# SessionEnd, so its last launch has no closing record anywhere in the file.
# Measured 2026-09-14: 4 of 1196 transcripts end their window on exactly that.
# Age is what makes it unreachable, and this is the seam that proves the
# shipped binary applies the bound — the unit test cannot see the clock the
# hook actually reads.
agent_launch_line toolu_qastale "$(iso_utc_at $((P5B_NOW - 7 * 3600)))" >>"$P5B_TAIL"
hook_fire Stop
check "a launch older than the bound never raises the mark" \
  "$(wait_field subagents false)" "false"

# --- and a dead session carries nothing, however the tail reads --------------
# `SessionEnd` reads no transcript at all, so it has no window to judge. It
# forces the mark down instead: nothing can still be pending under a session
# that has exited, and a held mark would sit on a dormant row claiming depth
# the user cannot go and look at.
#
# The mark is raised again FIRST. Firing SessionEnd on a row that is already
# clear asserts nothing — the check passed before this line was added, and
# would pass with the whole SessionEnd rule deleted.
agent_launch_line toolu_qalive "$(iso_utc_at "$P5B_NOW")" >>"$P5B_TAIL"
hook_fire Stop
check "the mark is up when the session dies, or SessionEnd proves nothing" \
  "$(wait_field subagents true)" "true"
hook_fire SessionEnd
# Main's wording: the mark is asserted DOWN, not "cleared" — SessionEnd forces
# it down whatever it was, so the check must not read as a transition.
check "SessionEnd leaves the subagent mark down" "$(wait_field subagents false)" "false"
check "and the row goes idle (nothing left Working into phase 6)" \
  "$(wait_field status idle)" "idle"

P5B_SEQ_END="$(jq -r '.store.seq' < <(dev_status) 2>/dev/null)"
check_numeric "phase-5b end store seq readable" "$P5B_SEQ_END"
measure "store seq across the card-cell ladder" \
  "before=${P5B_SEQ0} after=${P5B_SEQ_END} delta=$((P5B_SEQ_END - P5B_SEQ0))"
# Eight events, and a write per event is the ceiling — the hook is one locked
# RMW per event, the snapshot push is not a store write, and the warm-up above
# is what makes the count attributable by taking `pr-sync` out of it. More
# than that means something is writing twice per event, which is the shape the
# paced-12 check watches for on the toggle side. (Fewer is fine and expected:
# the background-command `Stop` re-asserts a state the row is already in.)
if [[ "$P5B_PR_WARM" == "fresh" ]]; then
  check "phase-5b writes per hook event <= 1 (8 events, delta <= 8)" \
    "$(((P5B_SEQ_END - P5B_SEQ0) <= 8 ? 1 : 0))" "1"
else
  note 'the PR cache never settled, so every event still spawns a `pr-sync` whose write lands off-schedule — the budget is not attributable here and is recorded above, not asserted'
fi
note 'this phase edited a scenario transcript — `clave dev scenario %s` re-seeds it' "$SCENARIO"
note 'this phase leaves two launches open in that transcript, so it is NOT idempotent within the six-hour age bound — re-run it through `just qa`, which re-seeds, not by calling this script again'

# ===========================================================================
# Phase 5c — the terminal row's facts (the OS-facts witness)
# ===========================================================================
# The card's terminal row shows what a plain shell tab is DOING: its cwd, its
# last command, and whether that command is still running. None of it comes
# from the store. Each bar keeps its own copy, filled three ways — zellij's
# `CwdChanged` and `CommandChanged` events, and an OS probe
# (`get_pane_cwd`/`get_pane_running_command`) that only the bar whose tab is
# focused pays for (main.rs `probe_term_facts`, gated on `own_tab_focused`).
#
# On 2026-09-12 the maintainer saw the same terminal row carry its facts under
# one tab and none under another. So this phase drives the two delivery paths
# that CAN be witnessed and asserts the property that was silently assumed:
#
#   Leg A — a `cd` in a real shell pane. Leg B — a real command run in it.
#   Both must reach EVERY LIVE BAR, not merely one. `CwdChanged` and
#   `CommandChanged` are ingested by every instance (main.rs, both arms
#   ungated), which is what makes the fleet-wide count an assertion rather
#   than a hope: gating those arms behind visibility would look like an
#   optimisation and would take the terminal row with it.
#
# Measured live before this phase existed (sandbox drive, 2026-09-12): one
# `sleep` in one shell tab produced a `term-facts command delta` line from all
# SEVEN bars in the fleet. That is the number this phase pins.
#
# WHAT NO LOG CAN SHOW, and the reason the fix is a separate PR: a bar that
# never learned a QUIET pane's facts. A pane already sitting at its prompt
# when that bar was born fires no event, and only the focused bar probes — so
# the instance simply has nothing, and "nothing happened" writes no line. That
# is the flicker's real shape, and it stops being invisible when the facts
# move into the store, where `collapsed` and `tab_order` both ended up after
# diverging the same way. Then this phase reads the row every bar renders,
# instead of the deliveries it can see.
phase "P5c-term-facts"

# The denominator is LIVE bars, one per tab (the layout's design, and the same
# reading phase 2 uses for the live block). Not the log's instance list: phase
# 3 closed two tabs, and their `clave-bar: loaded` lines outlive them, so the
# log's fleet is every bar this session EVER had.
P5C_LIVE_TABS="$(count_live_tabs)"
check_numeric "phase-5c live tab count (one bar per tab)" "$P5C_LIVE_TABS"
measure "bar instances in the log, live and dead (the log's fleet, for context)" \
  "$(sandbox_instance_count)"

# A terminal tab is one no agent is bound to — the same definition the bar
# renders from (model.rs `probe_targets` excludes tabs with an agent). Read
# from the store and the layout together, never from a tab name: phase 3's
# `ct.sh new-tab` creates exactly this, and its name is zellij's default.
P5C_PANES="$(ct_list_panes)"
P5C_PANES_RC=$?
check "ct.sh list-panes -t -c -j (phase 5c)" \
  "$([[ $P5C_PANES_RC -eq 0 ]] && echo ok || echo failed)" "ok"
P5C_TERM_TABS="$(jq -r --argjson panes "$P5C_PANES" '
  ([.store.agents[] | select(.tab_id != null) | .tab_id]) as $bound
  | [$panes[].tab_id] | unique
  | [.[] | select(. as $t | $bound | index($t) | not)] | join(",")' \
  < <(dev_status) 2>/dev/null)"
measure "terminal tabs (no agent bound)" "${P5C_TERM_TABS:-none}"
P5C_TAB="${P5C_TERM_TABS%%,*}"

# One leg: type a line, then count the LIVE bars that logged a fact delta for
# it. `instances_logging_since` takes its own sub-mark so the count is this
# leg's own — the run-long mark would answer with phase 3's traffic.
p5c_leg() {
  local label="$1" keys="$2" pattern="$3" mark learned ids
  mark="$(zlog_now)"
  "$CT" write-chars "$keys"
  "$CT" write 13
  learned=0
  for _ in $(seq 1 10); do
    learned="$(instance_count_logging_since "$mark" "$pattern")"
    ((learned >= P5C_LIVE_TABS)) && break
    sleep 1
  done
  ids="$(instances_logging_since "$mark" "$pattern" | tr '\n' ' ')"
  measure "$label bars that learned it" "${ids:-none} (live tabs: ${P5C_LIVE_TABS})"
  check_min "$label reaches a bar at all (the pipeline delivers)" "$learned" 1
  check "$label reaches EVERY live bar (both event arms are ungated by design)" \
    "$learned" "$P5C_LIVE_TABS"
}

if [[ -z "$P5C_TAB" ]]; then
  note 'no terminal tab in this fleet, so the OS-facts pipeline has nothing to report on. Phase 3 creates one; a run that skipped or lost it lands here. NOT a pass — the legs below did not run.'
else
  focus_tab_checked "$P5C_TAB" "phase-5c terminal tab"

  # THE WRITE GUARD, load-bearing. The legs below type into the focused pane,
  # and the one thing that must be unexpressible is typing into an agent:
  # `write-chars` at a claude prompt submits a turn. Three conditions, all
  # re-read immediately before the write rather than inherited from above —
  # the tab carries no agent in the store (checked in the selection), the
  # focused tab is the one we selected, and the focused pane's own process is
  # a SHELL from an allowlist. A denylist would let any unrecognised process
  # through, which is the wrong default for a keystroke.
  P5C_PANES="$(ct_list_panes)"
  P5C_PANES_RC=$?
  check "ct.sh list-panes -t -c -j (phase 5c write guard)" \
    "$([[ $P5C_PANES_RC -eq 0 ]] && echo ok || echo failed)" "ok"
  P5C_PROC="$(jq -r --argjson t "$P5C_TAB" \
    '[.[] | select(.tab_id == $t and .is_focused == true and .is_plugin == false) | .pane_command // ""] | first // ""' \
    <<<"$P5C_PANES" 2>/dev/null)"
  P5C_SHELL="$(basename -- "${P5C_PROC:-none}")"
  measure "the focused pane's process in tab ${P5C_TAB}" "${P5C_PROC:-empty}"
  P5C_WRITABLE="no"
  case "$P5C_SHELL" in
    zsh | bash | sh | fish | dash) P5C_WRITABLE="yes" ;;
  esac
  check "phase-5c focus is still on the terminal tab" "$(focused_tab_id)" "$P5C_TAB"

  if [[ "$P5C_WRITABLE" != "yes" ]]; then
    note 'the focused pane in tab %s runs `%s`, which is not in the shell allowlist — nothing is typed into a pane this drive cannot identify as a shell. The legs did not run.' \
      "$P5C_TAB" "${P5C_PROC:-empty}"
  else
    # --- leg A: the cwd half ------------------------------------------------
    # `/tmp` exists everywhere and belongs to nobody. `cd -` puts the pane
    # back, so the row this sandbox's eyeball checks still reads its repo.
    p5c_leg "a cd in a shell tab" "cd /tmp" 'clave-bar: term-facts cwd delta'
    "$CT" write-chars "cd -"
    "$CT" write 13

    # --- leg B: the running-command half ------------------------------------
    # `sleep` and nothing else: it changes the pane's foreground command (the
    # fact under test), touches no file, and ends by itself well inside this
    # phase — a command still running at phase 6 would arm the bar's 3s term
    # poll through the window that is meant to be idle.
    p5c_leg "a running command" "sleep 6" 'clave-bar: term-facts command delta'
    sleep 8
    note 'the `sleep` is over and the pane is back at its prompt in its original directory.'
  fi
fi

# ===========================================================================
# Phase 6 — quiescence
# ===========================================================================
# Idle, then prove nothing moved. Covers P17 (idle traffic), B19/B20 (the
# self-exciting render loops — a bar that repaints itself into activity shows
# up here as seq/evlog drift), drive-loop step 6.
#
# Attribution rule (QA-DRIVE "Delivery accounting"): the zellij log is
# user-global and is NEVER globally flat with a live maintainer fleet, so the
# asserted reading is the sandbox-attributable one — fresh `clave-bar:
# loaded` lines carrying THIS build's tag (a quiet fleet loads no new bar).
# Global growth is recorded as forensic, not asserted.
phase "P6-quiescence"

P6_WAIT="${CLAVE_QUIESCE_WAIT:-60}"
# Quiescence asserts on EQUALITY across a window, so a masked read is worse
# than a missing one: a dead dev_status or an unreadable evlog defaulted to
# 0 on both ends reads as perfectly flat (CodeRabbit, PR #202). Every input
# to a flatness check must prove it was actually read.
P6_STATUS="$(dev_status)"
P6_SEQ0="$(jq -r '.store.seq' <<<"$P6_STATUS" 2>/dev/null)"
check_numeric "quiescence start store seq readable" "$P6_SEQ0"
P6_EV0="$(wc -l <"$EVLOG" 2>/dev/null | tr -d ' ')"
check_numeric "quiescence start evlog readable" "$P6_EV0"
P6_BARS0="$(bar_loaded_count)"
P6_ZLINES0="$(wc -l <"$ZLOG" 2>/dev/null | tr -d ' ')" || P6_ZLINES0=0
measure "quiescence start (idling ${P6_WAIT}s)" "seq=${P6_SEQ0} evlog_lines=${P6_EV0} tagged_bars=${P6_BARS0}"
sleep "$P6_WAIT"

P6_SEQ1="$(jq -r '.store.seq' < <(dev_status) 2>/dev/null)"
check_numeric "quiescence end store seq readable" "$P6_SEQ1"
P6_EV1="$(wc -l <"$EVLOG" 2>/dev/null | tr -d ' ')"
check_numeric "quiescence end evlog readable" "$P6_EV1"
check "store seq flat across ${P6_WAIT}s idle" "$P6_SEQ1" "$P6_SEQ0"
check "evlog flat across ${P6_WAIT}s idle" "$P6_EV1" "$P6_EV0"
check "no new sandbox bar loaded while idle (tagged 'clave-bar: loaded' delta)" \
  "$(($(bar_loaded_count) - P6_BARS0))" "0"
P6_ZLINES1="$(wc -l <"$ZLOG" 2>/dev/null | tr -d ' ')" || P6_ZLINES1=0
measure "global zellij log growth while idle (user-global, unattributable — forensic only)" \
  "$((P6_ZLINES1 - P6_ZLINES0))"

# ===========================================================================
# Phase 6b — the isolation witness
# ===========================================================================
# The one property this whole harness rests on and never used to assert: the
# maintainer's own session was not touched. It cannot be checked by looking at
# his session — looking IS touching, and the rule is that nothing goes that
# way, not even a read. So it is checked from our side, where the evidence
# actually is:
#
#   1. clave refuses a push whose target does not own the store it wrote, and
#      writes one line when it does. Zero refusals means nothing this drive
#      spawned was even POINTED anywhere else.
#   2. The ambient identity is still the sandbox's at the end, not just at the
#      start — a phase that re-exported the inherited name would show here.
#   3. The inherited session's name appears nowhere as a push target.
#
# (1) is the load-bearing one, and it is only trustworthy because the refusal
# lives in the BINARY: a check that the script performs on itself would have
# passed happily on 2026-09-11, which is the day this phase was written.
phase "P6b-isolation-witness"

QA_REFUSED_AFTER="$(count_push_refusals)"
measure "push refusals during this run" \
  "before=${QA_REFUSED_BEFORE} after=${QA_REFUSED_AFTER} delta=$((QA_REFUSED_AFTER - QA_REFUSED_BEFORE))"
check "no push was aimed at a bar that does not own this store" \
  "$((QA_REFUSED_AFTER - QA_REFUSED_BEFORE))" "0"

check "the ambient zellij identity is STILL the sandbox at the end of the run" \
  "${ZELLIJ_SESSION_NAME:-<unset>}" "$SESSION"
check "ZELLIJ is still unset (nothing re-attached this shell to a session)" \
  "${ZELLIJ:-unset}" "unset"
check "ZELLIJ_PANE_ID is still unset" "${ZELLIJ_PANE_ID:-unset}" "unset"

if [[ -n "$FOREIGN_SESSION" && "$FOREIGN_SESSION" != "$SESSION" ]]; then
  measure "the identity this drive inherited and scrubbed" "$FOREIGN_SESSION"
  # Named targets are written into the refusal line; a clean log has none at
  # all, so this is a second reading of (1) from the other direction.
  QA_FOREIGN_HITS="$(grep -c "target=${FOREIGN_SESSION}" "$STATE_DIR/clave.log" 2>/dev/null)" || QA_FOREIGN_HITS=0
  check "the inherited session is named nowhere as a push target" \
    "${QA_FOREIGN_HITS:-0}" "0"
else
  note 'this drive inherited no foreign zellij identity — the scrub had nothing to do, and (3) is vacuous'
fi

# ===========================================================================
# Phase 6c — the relaunch (the second launch)
# ===========================================================================
# Every phase above runs inside ONE session. A relaunch is the one thing none
# of them can see: what the store carries across a session boundary, and what
# the next launch does with it. The live-set restore (#261) decayed all the
# way to a shipped branch with four gates green because nothing launched
# twice; a maintainer launching twice by hand was the only instrument. This
# phase is that instrument, automated.
#
# What a relaunch does now (setup.rs `launch_layout_kdl`, `eager_row`;
# decision of 2026-09-22, FOOTGUNS "The restore that sequenced tabs through
# the bar"): it bakes ONE tab, for the most-recent row whose cwd still exists.
# Nothing is held and nothing is restored. The rows the quit left live come
# back on STANDBY (decision of 2026-09-22): the quit's SessionEnd, or the
# launch when that hook was lost, stamped each one, and arriving on one opens it. Every other row is dormant, and
# Alt+Enter opens it.
#
# It never kills and never launches. Session lifecycle stays the human's
# (AGENTS.md), and `script_hygiene.rs` fails the build if any line here starts
# or ends a session. The phase prints the pair and waits, the same shape the
# first launch already uses at the top of this script.
#
# Two things make the readings non-vacuous, and both were measured in the
# source rather than assumed:
#
#   1. A launch CLEARS every tab_id and pane_id before it bakes the layout
#      (store.rs `clear_session_order`, called from `launch_session` when the
#      session is not live). So every bind read after the relaunch was made by
#      the second session. A stale bind cannot pass this phase for it.
#   2. The same pass resets Working and NeedsYou to Idle, because a launch
#      starts no agent. So a row wearing either after the relaunch is a claim
#      the second session made about a process that is not there.
#
# The phase leans on phase 6 above: quiescence has just proved the store is
# flat, so the snapshot read here is still the store at the moment of the quit.
phase "P6c-relaunch"

P6C_BEFORE_STATUS="$(dev_status)"
P6C_SET_BEFORE="$(bound_uuids "$P6C_BEFORE_STATUS")"
P6C_N_BEFORE="$(uuid_count "$P6C_SET_BEFORE")"
P6C_SEQ_BEFORE="$(jq -r '.store.seq' <<<"$P6C_BEFORE_STATUS" 2>/dev/null)"
check_numeric "the bound set is readable before the quit" "$P6C_N_BEFORE"
measure "the bound set this session is holding" "n=${P6C_N_BEFORE} $(uuid_line "$P6C_SET_BEFORE")"

# The expectation is computed HERE, from the snapshot the quit will leave,
# because that is the store the launch reads (`eager_row`). Computing it after
# the relaunch would read a store the second session has already written to,
# and the verdict would be measuring its own memory.
P6C_EXPECTED="$(eager_candidate_uuid "$P6C_BEFORE_STATUS")"
check_nonempty "the pre-quit snapshot names the row the relaunch will bake" "$P6C_EXPECTED"
measure "the row the relaunch should bake (most recent, cwd exists)" "$P6C_EXPECTED"
P6C_STANDBY_EXPECTED="$(standby_expected_uuids "$P6C_BEFORE_STATUS" "$P6C_EXPECTED")"
measure "the rows the quit should leave on standby" "$(uuid_line "$P6C_STANDBY_EXPECTED")"

# Half an hour for each half, measured rather than guessed: the first live run
# (2026-09-16) asked for the quit at ten minutes, the maintainer had stepped
# away, and the window closed. Ten green phases went with it. The ask sits at
# the END of a twelve-minute drive, so whoever started it has stopped watching
# by the time it arrives — the budget has to fit somebody coming back to it.
P6C_WAIT="${QA_RELAUNCH_WAIT:-1800}"
cat <<EOF

==> PHASE 6c needs a SECOND launch from you, and that is the whole point.

    The agent that staged this drive quits the sandbox by name; then the
    maintainer launches it again. Change NOTHING in between, and touch
    nothing after — the phase reads the fleet the launch bakes by itself:
    one tab, the rows left live on standby, the rest dormant.

    agent:       zellij kill-session ${SESSION}
                 zellij delete-session --force ${SESSION}
    maintainer:  cd ${ROOT}
                 just launch

    The drive waits up to ${P6C_WAIT}s for the session to go down and come
    back, then reads the store. It kills nothing itself.

EOF

# Each leg gets the FULL budget. They are two separate human actions with a
# pause between them, and sharing one countdown would spend the relaunch's
# patience on however long the quit took.
P6C_DOWN=0
read_liveness
while [[ "$SESSION_LIVE" == "true" && "$P6C_DOWN" -lt "$P6C_WAIT" ]]; do
  sleep 2
  P6C_DOWN=$((P6C_DOWN + 2))
  read_liveness
done
P6C_RAN="yes"
if [[ "$SESSION_LIVE" == "true" ]]; then
  # NOT RUN, not FAILED. The seam is unmeasured either way, but a phase that
  # never got its precondition has no verdict to give, and calling it red
  # would hide which phases are actually red. The run carries on to the
  # hand-back so the ten phases above it survive a maintainer who stepped out.
  skip_phase "$(printf '%s was still live after %ss — nobody quit it. The relaunch seam is UNMEASURED this run, and no other phase covers it.' "$SESSION" "$P6C_WAIT")"
  P6C_RAN="no"
fi
if [[ "$P6C_RAN" == "yes" ]]; then
  measure "the first session is down" "after ${P6C_DOWN}s"

  P6C_UP=0
  while [[ "$SESSION_LIVE" != "true" && "$P6C_UP" -lt "$P6C_WAIT" ]]; do
    sleep 2
    P6C_UP=$((P6C_UP + 2))
    read_liveness
  done
  if [[ "$SESSION_LIVE" != "true" ]]; then
    skip_phase "$(printf '%s did not come back within %ss. This is a missing launch, not a finding about the relaunch — but the seam is UNMEASURED, and the sandbox is DOWN, so the eyeball checkpoints below have nothing to look at either.' "$SESSION" "$P6C_WAIT")"
    P6C_RAN="no"
  fi
fi

if [[ "$P6C_RAN" == "yes" ]]; then
  measure "the second session is up" "after ${P6C_UP}s"

  # The settle window. Bounded polling rather than a fixed sleep: the baked
  # tab registers its bind when its spawn runs, which is after the session is
  # up. The loop waits for ONE bound row and no longer, so a launch that bakes
  # nothing still falls through to the checks below and fails loudly rather
  # than waiting forever. The store is read once per turn and ALWAYS at least
  # once, so the verdict can never run on an unset reading.
  P6C_SETTLE="${QA_RELAUNCH_SETTLE:-30}"
  P6C_SETTLED=0
  while :; do
    P6C_AFTER_STATUS="$(dev_status)"
    [[ "$(uuid_count "$(bound_uuids "$P6C_AFTER_STATUS")")" -ge 1 ]] && break
    ((P6C_SETTLED >= P6C_SETTLE)) && break
    sleep 2
    P6C_SETTLED=$((P6C_SETTLED + 2))
  done
  measure "the relaunched fleet settled" "after ${P6C_SETTLED}s (nothing was driven, focused or typed)"
  measure "store seq across the relaunch" \
    "before=${P6C_SEQ_BEFORE} after=$(jq -r '.store.seq' <<<"$P6C_AFTER_STATUS" 2>/dev/null)"

  # The verdict lives in qa/lib.sh, where the selftest runs it against a store
  # with two bound rows, the wrong row bound, and a stale status, and requires
  # each to go red. A comparison written the wrong way round here would pass
  # on every run, and the next person to learn otherwise would be a maintainer
  # launching twice — the loop this phase replaces.
  relaunch_checks "$P6C_EXPECTED" "$P6C_AFTER_STATUS" "$P6C_STANDBY_EXPECTED"

  # The beacon after the relaunch. This is the seam that caught the flap
  # under the old restore (devbox, 2026-09-22): every restored tab was born
  # focused, so the beacon ended on the LAST tab built while zellij's focus
  # rested on the first, and Alt+c in the first tab asked nothing while the
  # last tab's bar asked for it. One baked tab has no such race, and this
  # block is what proves it: the seam stays, verbatim, so a regression that
  # opens a second tab at launch is seen here.
  #
  # NO `anchor_executor` here, on purpose: phase 5 pipes the beacon before it
  # presses, and that hides exactly this defect. The launch itself must leave
  # the beacon where the focus is. Two presses, so the phase leaves `collapsed`
  # where it found it; each must be ONE ask from the bar in the focused tab,
  # painted by that same bar. A cooldown re-ask (`source=cooldown`) or an ask
  # naming another tab is the flap.
  P6C_STAND="$(focused_tab_id)"
  check_nonempty "post-relaunch standing tab (focus read, nothing driven)" "$P6C_STAND"
  P6C_TOGGLE_MARK="$(zlog_now)"
  P6C_TOGGLE_EXPECT="$(jq -r '.store.collapsed // false' <<<"$P6C_AFTER_STATUS" 2>/dev/null)"
  for i in 1 2; do
    if [[ "$P6C_TOGGLE_EXPECT" == "true" ]]; then P6C_TOGGLE_EXPECT="false"; else P6C_TOGGLE_EXPECT="true"; fi
    toggle_pipe
    P6C_RC=$?
    check "post-relaunch press ${i}/2 pipe accepted" "$([[ $P6C_RC -eq 0 ]] && echo ok || echo failed)" "ok"
    check "post-relaunch press ${i}/2 landed (store collapsed flipped)" "$(wait_collapsed "$P6C_TOGGLE_EXPECT")" "$P6C_TOGGLE_EXPECT"
    sleep 2
  done
  check "post-relaunch presses made one width ask each (a cooldown re-ask is an ask that landed on another tab)" \
    "$(swap_ask_count_since "$P6C_TOGGLE_MARK")" "2"
  # The render-path ask names its tab; a cooldown re-ask does not, so the
  # count above is what sees a cooldown and this line is what sees the WRONG
  # bar asking.
  check "post-relaunch asks named the standing tab ${P6C_STAND} as their own (the beacon rests where the focus is)" \
    "$(sandbox_lines_since "$P6C_TOGGLE_MARK" 'clave-bar: swap-width' | grep -c "tab=Some(${P6C_STAND})" || true)" "2"
  # Both sides are id sets. Two empty sets compare equal, so the asking set is
  # pinned non-empty first; without that the line could stand alone and pass
  # on a fleet that logged nothing.
  P6C_ASKERS="$(instances_logging_since "$P6C_TOGGLE_MARK" 'clave-bar: swap-width' | tr '\n' ' ')"
  check_nonempty "post-relaunch at least one sandbox instance asked" "$P6C_ASKERS"
  check "post-relaunch paints came from the bar that asked (the swap landed on the tab that asked for it)" \
    "$(instances_logging_since "$P6C_TOGGLE_MARK" 'clave-bar: painted' | tr '\n' ' ')" \
    "$P6C_ASKERS"

  # The arrival leg. One Alt+Down from the baked tab lands on the top standby
  # row, and the landing opens it with no Alt+Enter. It runs AFTER the beacon
  # leg, because a nav press moves the beacon that leg reads. The wait is for
  # a second bound row, the same bounded shape as the settle above: an open
  # that never binds falls through to the verdict and fails there.
  P6C_STANDBY_NOW="$(standby_uuids "$P6C_AFTER_STATUS")"
  nav_pipe '{"dir":"next"}'
  P6C_ARRIVAL=0
  while :; do
    P6C_ARRIVED_STATUS="$(dev_status)"
    [[ "$(uuid_count "$(bound_uuids "$P6C_ARRIVED_STATUS")")" -ge 2 ]] && break
    ((P6C_ARRIVAL >= P6C_SETTLE)) && break
    sleep 2
    P6C_ARRIVAL=$((P6C_ARRIVAL + 2))
  done
  measure "one Alt+Down from the baked tab settled" "after ${P6C_ARRIVAL}s"
  arrival_checks "$P6C_EXPECTED" "$P6C_STANDBY_NOW" "$P6C_ARRIVED_STATUS"
fi

# ===========================================================================
# Phase 7 — teardown (the hand-back)
# ===========================================================================
# Asserts nothing, launches nothing, kills nothing: session lifecycle is the
# agent's and the maintainer's, never the drive's (AGENTS.md). The drive's
# last act is to print the kill pair and the two eyeball checkpoints it owes.
phase "P7-teardown"

measure "sandbox left as driven; store, evlog and drive log preserved for forensics" "$STATE_DIR"
if [[ "${P6C_RAN:-no}" == "yes" ]]; then
  cat <<EOF

The session in front of you is the SECOND one, relaunched by phase 6c, so
checkpoint 1 below reads the RELAUNCHED fleet: the baked tab and the one
Alt+Down opened, then the standby rows (half-filled circle), then the
dormant rows (hollow circle).
EOF
fi
cat <<EOF

The two eyeball checkpoints (human, one message each — QA-DRIVE):
  1. one bar per tab; woken rows show agent chips, not terminal glyphs
  2. every tab a strip (or every tab wide) — no width outliers

Teardown, by the agent that staged this sandbox, once both eyeballs are in:
  zellij kill-session ${SESSION}
  zellij delete-session --force ${SESSION}
EOF

print_summary
# The readings first, then the truth about what is missing from them.
exit_on_incomplete
