#!/usr/bin/env bash
# qa-drive.sh — the automated regression drive, phases 0-2 (docs/dev/QA-DRIVE.md).
#
# What this is: preflight, baseline join, and the dormant-row bind ladder,
# scripted against THIS checkout's per-worktree sandbox instance. Phases 3-7
# (churn, ring walk, collapse, quiescence, teardown) are not built yet — see
# the build order in docs/dev/QA-DRIVE.md.
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
CLAVE_BIN="${CLAVE_BIN:-$ROOT/target/release/clave}"

usage() {
  cat <<EOF
usage: $0 <scenario>

Drives QA-DRIVE phases 0-2 (preflight, baseline join, bind ladder) against
THIS checkout's per-worktree sandbox instance (\`clave dev instance\`). Never
launches or kills a zellij session — stage and launch first:

  just sandbox <scenario>
  clave dev scenario <scenario>   # if not already seeded by \`just sandbox\`
  (human, non-zellij terminal) clave dev launch

then run this. Refuses closed if the instance's sandbox session is not live.

  <scenario>   the scenario name already staged/launched (e.g. qa-fleet).
               Informational for the report header and for phase 1's exact
               row-count expectation, which is currently known for
               \`qa-fleet\` only — other scenarios still run phases 0-2 but
               phase 1's row-count checks fall back to measurement-only.
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

# `clave dev status` is liveness-gated by construction (TESTING.md, "the
# observability map") — safe to call even against a dead session, unlike a
# bare `zellij action`, which blocks indefinitely against one. This is the
# fail-closed refusal: the drive never proceeds against a session that is
# not actually up.
STATUS_JSON="$("$CLAVE_BIN" dev status 2>/dev/null)" || STATUS_JSON=""
SESSION_LIVE="$(printf '%s' "$STATUS_JSON" | jq -r '.session_live // false' 2>/dev/null)"
if [[ "$SESSION_LIVE" != "true" ]]; then
  cat >&2 <<EOF
REFUSING: sandbox session '${SESSION}' is not live.

This drive never launches a session — stage and launch first:
  just sandbox ${SCENARIO}
  clave dev scenario ${SCENARIO}   # if not already seeded by \`just sandbox\`
  (human, non-zellij terminal) clave dev launch
then re-run: $0 ${SCENARIO}
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

# The mark: everything phase 1+ reads from the zellij log is lines AFTER
# this point, taken at THIS script's start. Phase 0's build-tag check is the
# deliberate exception — see the comment at that check.
if [[ -r "$ZLOG" ]]; then
  LOGMARK="$(wc -l <"$ZLOG" | tr -d ' ')"
else
  LOGMARK=0
fi
zlog_tail() {
  if [[ -r "$ZLOG" ]]; then
    tail -n "+$((LOGMARK + 1))" "$ZLOG"
  fi
}

# The build tag `just sandbox` baked (sandbox-setup.sh derives it from
# `git rev-parse --short HEAD` in the checkout that staged it — same
# derivation here, from THIS checkout).
BUILD_TAG="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo dev)"

# ---------------------------------------------------------------------------
# phase()/check()/measure() — the tracing spec's helpers.
# ---------------------------------------------------------------------------

CURRENT_PHASE=""
PHASE_NAMES=()
PHASE_RESULTS=()

ts() { date '+%H:%M:%S'; }

phase() {
  CURRENT_PHASE="$1"
  PHASE_NAMES+=("$1")
  PHASE_RESULTS+=("PASS")
  printf '\n[%s %s] PHASE START\n' "$CURRENT_PHASE" "$(ts)"
}

# A recorded reading — not an assertion. "empty" is printed as the word.
measure() {
  local desc="$1" val="${2:-}"
  [[ -z "$val" ]] && val="empty"
  printf '[%s %s] MEASURE %s: %s\n' "$CURRENT_PHASE" "$(ts)" "$desc" "$val"
}

print_summary() {
  echo
  echo "== QA drive summary (${SCENARIO}) =="
  local i
  for i in "${!PHASE_NAMES[@]}"; do
    printf '  %-18s %s\n' "${PHASE_NAMES[$i]}" "${PHASE_RESULTS[$i]}"
  done
  echo "log: ${DRIVE_LOG}"
  echo "Phases 3-7 (churn, ring, collapse, quiescence, teardown) are not yet built."
}

# Mark the current phase FAILED, print the summary, and stop the run. The
# log and sandbox are left exactly as they are — forensics, not a re-run.
fail_phase() {
  local last=$((${#PHASE_RESULTS[@]} - 1))
  PHASE_RESULTS[last]="FAIL"
  printf '\nPHASE %s FAILED\n' "$CURRENT_PHASE"
  print_summary
  exit 1
}

# check <desc> <measured> <expected> — exact string match. First FAIL stops
# the run (fail_phase exits non-zero).
check() {
  local desc="$1" measured="${2:-}" expected="${3:-}"
  [[ -z "$measured" ]] && measured="empty"
  [[ -z "$expected" ]] && expected="empty"
  if [[ "$measured" == "$expected" ]]; then
    printf '[%s %s] CHECK %s: measured=%s expected=%s PASS\n' "$CURRENT_PHASE" "$(ts)" "$desc" "$measured" "$expected"
  else
    printf '[%s %s] CHECK %s: measured=%s expected=%s FAIL\n' "$CURRENT_PHASE" "$(ts)" "$desc" "$measured" "$expected"
    fail_phase
  fi
}

# check_min <desc> <measured-int> <min-int> — measured >= min.
check_min() {
  local desc="$1" measured="${2:-}" min="$3" verdict="FAIL"
  if [[ "$measured" =~ ^[0-9]+$ ]] && ((measured >= min)); then
    verdict="PASS"
  fi
  printf '[%s %s] CHECK %s: measured=%s expected=>=%s %s\n' "$CURRENT_PHASE" "$(ts)" "$desc" "${measured:-empty}" "$min" "$verdict"
  [[ "$verdict" == "FAIL" ]] && fail_phase
}

# check_nonempty <desc> <measured> — measured must be non-blank (used where
# the expected value is only known once measured, e.g. a bound tab_id).
check_nonempty() {
  local desc="$1" measured="${2:-}" verdict="FAIL"
  [[ -n "$measured" ]] && verdict="PASS"
  printf '[%s %s] CHECK %s: measured=%s expected=<non-empty> %s\n' "$CURRENT_PHASE" "$(ts)" "$desc" "${measured:-empty}" "$verdict"
  [[ "$verdict" == "FAIL" ]] && fail_phase
}

dev_status() { "$CLAVE_BIN" dev status 2>/dev/null; }

# Guarded list-panes read. Never the bare env-var form (TESTING.md, "the
# sandbox drive loop" step — a dead/absent session hangs `zellij action`
# forever; ct.sh bounds it). Returns "[]" and a non-zero status on any
# failure so callers can jq it unconditionally — but "[]" is a VALID empty
# panes list too, so a caller that only looks at the JSON and not the
# return code cannot tell a genuine empty read from a ct.sh refusal
# (FOOTGUNS, "the wrapper's refusal is the only thing it prints" — a
# swallowed stderr here is exactly that trap). ct.sh's own stderr is
# deliberately NOT redirected to /dev/null: it flows to this script's fd2,
# which is already teed into DRIVE_LOG by the top-level `exec` redirect, so
# a refusal is never discarded. Every caller MUST check the return code.
ct_list_panes() {
  local out
  if ! out="$("$CT" list-panes -t -j)"; then
    printf '[%s %s] ct.sh list-panes -t -j FAILED (stderr above)\n' "$CURRENT_PHASE" "$(ts)" >&2
    echo "[]"
    return 1
  fi
  if ! jq -e . >/dev/null 2>&1 <<<"$out"; then
    printf '[%s %s] ct.sh list-panes -t -j returned non-JSON: %s\n' "$CURRENT_PHASE" "$(ts)" "$out" >&2
    echo "[]"
    return 1
  fi
  printf '%s' "$out"
}

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
LOADED_LINES="$(grep -F 'clave-bar: loaded' "$ZLOG" 2>/dev/null || true)"
LOADED_TAIL="$(printf '%s\n' "$LOADED_LINES" | tail -5)"
measure "loaded-tail (last 5)" "$LOADED_TAIL"
LAST_LOADED="$(printf '%s\n' "$LOADED_LINES" | tail -1)"
measure "loaded-tail matched line (verbatim)" "$LAST_LOADED"
LAST_BUILD="$(printf '%s' "$LAST_LOADED" | grep -oE 'build=[^ ]+' | cut -d= -f2)"
check "build tag on loaded tail" "$LAST_BUILD" "$BUILD_TAG"
printf '[%s %s] NOTE same-HEAD re-runs cannot distinguish a stale load here — eyeball the tail timestamps\n' "$CURRENT_PHASE" "$(ts)"

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
ORPHANS="$(pgrep -f 'zellij pipe' 2>/dev/null || true)"
check "orphan 'zellij pipe' processes" "$ORPHANS" ""

# ===========================================================================
# Phase 1 — baseline join
# ===========================================================================
phase "P1-baseline-join"

STATUS_JSON="$(dev_status)"
measure "dev status (raw)" "$STATUS_JSON"

TOTAL_ROWS="$(jq '.store.agents | length' <<<"$STATUS_JSON" 2>/dev/null)"
DORMANT_COUNT="$(jq '[.store.agents[] | select(.tab_id == null)] | length' <<<"$STATUS_JSON" 2>/dev/null)"
measure "total rows" "$TOTAL_ROWS"
measure "dormant rows (tab_id null)" "$DORMANT_COUNT"

if [[ "$SCENARIO" == "qa-fleet" ]]; then
  # qa-fleet seeds 6 dormant rows; cold start's eager-launch selection
  # (setup.rs `eager_row` — the most-recent row whose cwd still exists)
  # auto-resumes exactly one of them into a live tab, so the STEADY STATE
  # this drive measures is 6 total / 5 still dormant / 1 bound.
  check "total rows == scenario seed count" "$TOTAL_ROWS" "6"
  check "dormant rows after eager resume" "$DORMANT_COUNT" "5"
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
check "ct.sh list-panes -t -j (join/viewport)" "$([[ $PANES_RC -eq 0 ]] && echo ok || echo failed)" "ok"
VIEWPORT="$(jq -c '[.[] | select(.pane_info.is_plugin == true) | {tab_id, columns: .pane_info.pane_content_columns, rows: .pane_info.pane_content_rows}]' <<<"$PANES_JSON" 2>/dev/null)"
measure "viewport geometry (bar panes, via ct.sh)" "$VIEWPORT"

# Store <-> layout join, unresolvables MARKED, never filtered (TESTING.md,
# "the join is not as easy as it looks" — `pane_command` is the pane's
# DEEPEST child process, so an agent pane routinely shows something other
# than `claude` and that is unknown, not a mismatch).
JOIN="$(jq -c --argjson panes "$PANES_JSON" '
  [ .store.agents | to_entries[] | select(.value.tab_id != null) |
    . as $e |
    ($panes | map(select(.tab_id == ($e.value.tab_id) and (.pane_info.is_plugin | not)))) as $cands |
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

# Live clave-bar plugin panes = live bar instances, the EOF-twin multiplier
# (one twin per instance per pipe, P8). NOT a `ct_list_panes | jq …` pipe:
# with `pipefail` set, a failed ct_list_panes (which still prints valid
# empty-JSON "[]" so jq itself succeeds) would leave the pipeline's exit
# status 0 — the exact swallowed-refusal trap this rewrite exists to close.
# Capture the panes read and its status separately instead.
count_live_instances() {
  local panes
  panes="$(ct_list_panes)" || return 1
  jq '[.[] | select(.pane_info.is_plugin == true and ((.pane_info.plugin_url // "") | test("clave-bar")))] | length' <<<"$panes" 2>/dev/null
}

count_dormant() {
  jq '[.store.agents[] | select(.tab_id == null)] | length' < <(dev_status) 2>/dev/null
}

count_eof_twins() {
  zlog_tail | grep -c -F 'clave-bar: dropped' 2>/dev/null || true
}

RUNG=0
ATTEMPTED=0
LANDED=0

while :; do
  CUR_STATUS="$(dev_status)"
  DORMANT_LIST="$(jq -r '.store.agents | to_entries[] | select(.value.tab_id == null) | .key' <<<"$CUR_STATUS" 2>/dev/null)"
  [[ -z "$DORMANT_LIST" ]] && break
  RUNG=$((RUNG + 1))
  UUID_TARGET="$(printf '%s\n' "$DORMANT_LIST" | head -n1)"
  DORMANT_BEFORE="$(printf '%s\n' "$DORMANT_LIST" | grep -c .)"
  LIVE_BEFORE="$(count_live_instances)"
  LIVE_RC=$?
  check "ct.sh list-panes -t -j (rung $RUNG live-instance count)" "$([[ $LIVE_RC -eq 0 ]] && echo ok || echo failed)" "ok"
  TWINS_BEFORE="$(count_eof_twins)"
  ATTEMPTED=$((ATTEMPTED + 1))

  if ((RUNG == 1)); then
    # The scripted-create leg of the mixed-path ladder: the CLI open path
    # directly, bypassing nav pipes entirely. Explicit instance env,
    # mirroring ct.sh's scoping discipline for `zellij action` — `clave
    # open` reads CLAVE_SESSION/CLAVE_STATE_DIR/CLAVE_DATA_DIR, and unset
    # they default to the MAINTAINER's real session and store (env.rs).
    METHOD="scripted-open"

    # Re-verify liveness immediately before this call, not just once at
    # script start: `clave open`'s internal zellij invocations are
    # UNBOUNDED (open.rs — the dump-layout read at :74-78 and the new-tab
    # spawn at :148-158 both call `.output()`/`.status()` with no timeout),
    # so a session that died between phase start and here would wedge this
    # call forever with no signal at all.
    PREOPEN_LIVE="$(jq -r '.session_live // false' <<<"$(dev_status)" 2>/dev/null)"
    check "session live immediately before scripted-open" "$PREOPEN_LIVE" "true"

    # Bound the call externally, since open.rs cannot bound itself: prefer
    # coreutils `timeout`/`gtimeout` (same idiom as ct.sh); fall back to a
    # bash-native watchdog (background killer racing the foreground call)
    # when neither is on PATH.
    OPEN_TIMEOUT="${CLAVE_OPEN_TIMEOUT:-30}"
    if command -v timeout >/dev/null 2>&1; then
      CLAVE_SESSION="$SESSION" CLAVE_STATE_DIR="$STATE_DIR" CLAVE_DATA_DIR="$DATA_DIR" \
        timeout "$OPEN_TIMEOUT" "$CLAVE_BIN" open "$UUID_TARGET"
      OPEN_RC=$?
    elif command -v gtimeout >/dev/null 2>&1; then
      CLAVE_SESSION="$SESSION" CLAVE_STATE_DIR="$STATE_DIR" CLAVE_DATA_DIR="$DATA_DIR" \
        gtimeout "$OPEN_TIMEOUT" "$CLAVE_BIN" open "$UUID_TARGET"
      OPEN_RC=$?
    else
      ( CLAVE_SESSION="$SESSION" CLAVE_STATE_DIR="$STATE_DIR" CLAVE_DATA_DIR="$DATA_DIR" \
        "$CLAVE_BIN" open "$UUID_TARGET" ) &
      OPEN_PID=$!
      ( sleep "$OPEN_TIMEOUT"; kill -TERM "$OPEN_PID" 2>/dev/null ) &
      WATCHDOG_PID=$!
      wait "$OPEN_PID"
      OPEN_RC=$?
      kill "$WATCHDOG_PID" 2>/dev/null
      wait "$WATCHDOG_PID" 2>/dev/null
    fi

    if ((OPEN_RC == 0)); then
      OPEN_RESULT="ok"
    elif ((OPEN_RC == 124)) || ((OPEN_RC == 143)); then
      # 124: GNU `timeout`'s own exit code on expiry. 143 (128+SIGTERM):
      # the bash-native watchdog's kill signal reaching the child.
      OPEN_RESULT="timeout=${OPEN_TIMEOUT}s"
    else
      OPEN_RESULT="exit=${OPEN_RC}"
    fi
    measure "rung $RUNG ($METHOD) result" "$OPEN_RESULT"
    if [[ "$OPEN_RESULT" == timeout=* ]]; then
      printf '\n[%s %s] WEDGE: clave open %s did not return within %ss — open.rs'"'"'s internal zellij calls are unbounded; this external timeout is the only bound this drive has.\n' \
        "$CURRENT_PHASE" "$(ts)" "$UUID_TARGET" "$OPEN_TIMEOUT"
      fail_phase
    fi
    # `clave open` runs `zellij action new-tab`, not `zellij pipe` — no
    # CliPipe broadcast, so no EOF-twin traffic is attributable to it.
    EXPECTED_TWINS=0
  else
    METHOD="nav-pick-commit"
    # Rows render live block first (low numbers), dormant block after
    # (model.rs nav() doc comment) — so live_count+1 always addresses the
    # topmost REMAINING dormant row regardless of within-block order.
    LIVE_NOW="$(jq '[.store.agents[] | select(.tab_id != null)] | length' <<<"$CUR_STATUS" 2>/dev/null)"
    ROW=$((LIVE_NOW + 1))
    "$CT" pipe --name clave-nav -- "{\"row\":${ROW}}"
    "$CT" pipe --name clave-nav -- '{"commit":true}'
    measure "rung $RUNG ($METHOD) row picked" "$ROW"
    EXPECTED_TWINS=$((LIVE_BEFORE * 2))
  fi

  # Bounded wait (10s poll): tab_id lands in store.
  BOUND_TID=""
  for _ in $(seq 1 10); do
    BOUND_TID="$(jq -r --arg u "$UUID_TARGET" '.store.agents[$u].tab_id // empty' < <(dev_status) 2>/dev/null)"
    [[ -n "$BOUND_TID" ]] && break
    sleep 1
  done
  check_nonempty "rung $RUNG tab_id bound uuid=${UUID_TARGET:0:13}" "$BOUND_TID"
  [[ -n "$BOUND_TID" ]] && LANDED=$((LANDED + 1))

  # Dormant count decremented on the next snapshot.
  DORMANT_AFTER="$(count_dormant)"
  check "rung $RUNG dormant count decremented" "$DORMANT_AFTER" "$((DORMANT_BEFORE - 1))"

  # EOF-twin delta exact: pipes-sent x live-instances.
  TWINS_AFTER="$(count_eof_twins)"
  TWIN_DELTA=$((TWINS_AFTER - TWINS_BEFORE))
  check "rung $RUNG EOF-twin delta" "$TWIN_DELTA" "$EXPECTED_TWINS"

  # Seek-trace resting width — model BELIEF, not pane truth (the eyeball
  # stays the oracle). The trace needs instrumentation not present in the
  # shipped bar (seek-trace.sh's own header): on an uninstrumented build
  # this is legitimately unavailable, which is a PASS-with-note, not a FAIL.
  SEEK_LINE=""
  if [[ -x "$SCRIPT_DIR/seek-trace.sh" ]]; then
    SEEK_LINE="$("$SCRIPT_DIR/seek-trace.sh" tail 1 2>/dev/null | tail -1)"
  fi
  if [[ -n "$SEEK_LINE" ]]; then
    measure "rung $RUNG width belief (seek-trace, uninstrumented builds will not see this)" "$SEEK_LINE"
    check "rung $RUNG width belief recorded" "recorded" "recorded"
  else
    check "rung $RUNG width belief" "unavailable" "unavailable"
  fi

  printf '[%s %s] BUDGET rung %d: attempted=%d landed=%d (the "2 then never again" signature is #178'"'"'s tell)\n' \
    "$CURRENT_PHASE" "$(ts)" "$RUNG" "$ATTEMPTED" "$LANDED"
done

measure "bind ladder totals" "attempted=${ATTEMPTED} landed=${LANDED}"

print_summary
