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
# `-c` (final review BLOCKER 1): `pane_command` is
# `#[serde(skip_serializing_if = "Option::is_none")]` on zellij's
# `PaneListEntry` and is only populated when `list-panes` is asked for
# running-command info — without it every join below reads `pane_command`
# as absent and resolves UNRESOLVED unconditionally.
ct_list_panes() {
  local out
  if ! out="$("$CT" list-panes -t -c -j)"; then
    printf '[%s %s] ct.sh list-panes -t -c -j FAILED (stderr above)\n' "$CURRENT_PHASE" "$(ts)" >&2
    echo "[]"
    return 1
  fi
  if ! jq -e . >/dev/null 2>&1 <<<"$out"; then
    printf '[%s %s] ct.sh list-panes -t -c -j returned non-JSON: %s\n' "$CURRENT_PHASE" "$(ts)" "$out" >&2
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
#
# And the log is CROSS-SESSION (see count_eof_twins / FOOTGUNS): a tab
# opened in the maintainer's live fleet DURING this preflight appends a
# release-tagged loaded line, so "the very last line is ours" is a race.
# The check therefore accepts the tag anywhere in the 5-line tail — still
# tail-bounded, never the forbidden whole-file grep -c.
LOADED_LINES="$(grep -F 'clave-bar: loaded' "$ZLOG" 2>/dev/null || true)"
LOADED_TAIL="$(printf '%s\n' "$LOADED_LINES" | tail -5)"
measure "loaded-tail (last 5)" "$LOADED_TAIL"
TAIL_MATCH="$(printf '%s\n' "$LOADED_TAIL" | grep -F "build=$BUILD_TAG" | tail -1)"
measure "loaded-tail matched line (verbatim)" "$TAIL_MATCH"
check "build tag on loaded tail (any of last 5)" "$([[ -n "$TAIL_MATCH" ]] && echo present || echo absent)" "present"
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
EAGER_LS="$(jq -r --arg u "$EAGER_UUID" '.store.agents[$u].live_session // empty' <<<"$STATUS_JSON" 2>/dev/null)"
check_nonempty "eager row live_session (rotated transcript seeded)" "$EAGER_LS"
EAGER_CMD="$(jq -r --argjson t "${EAGER_TID:-null}" '[.[] | select(.tab_id == $t and ((.pane_command // "") | test("claude")))] | .[0].pane_command // empty' <<<"$PANES_JSON" 2>/dev/null)"
if [[ -n "$EAGER_CMD" ]]; then
  check "eager resume targets live_session, not the minted uuid" \
    "$([[ "$EAGER_CMD" == *"--resume ${EAGER_LS}"* ]] && echo ok || echo "mismatch: $EAGER_CMD")" "ok"
else
  measure "eager resume identity" "unresolvable — no pane_command in tab $EAGER_TID names claude (deepest-child, unknown not mismatched)"
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

# There is NO count_live_instances: the bar's panes are invisible to
# `list-panes`. Measured on the 2026-08-13 run: two instances provably
# loaded (fresh `clave-bar: loaded` lines, ids 1 and 2), zero clave-bar
# panes in the list — the only plugin pane it showed was zellij's own
# built-in `zellij:link`. Likely mechanism: the bar sets
# `set_selectable(false)` (FOOTGUNS "cannot be focused by clicking") and
# list-panes appears to serialize selectable panes only — unverified,
# zellij-server is not vendored. Live bar instances are therefore counted
# as live TABS (`count_live_tabs`): one bar per tab is the layout's design,
# and the loaded-line evidence above confirmed it (tab 1's bar loaded the
# instant `clave add` created the tab).
#
# NOT a `ct_list_panes | jq …` pipe: with `pipefail` set, a failed
# ct_list_panes (which still prints valid empty-JSON "[]" so jq itself
# succeeds) would leave the pipeline's exit status 0 — the exact
# swallowed-refusal trap this rewrite exists to close. Capture the panes
# read and its status separately instead.
#
# Selectors are FLAT (final review BLOCKER 1) — see the comment on
# `ct_list_panes` above.
#
# The live block's TRUE size, for ROW arithmetic (BLOCKER 2 finding 2):
# counting store rows with `tab_id != null` undercounts it the moment the
# sandbox also carries a plain terminal tab with no agent behind it, since
# model.rs's live block is one row per zellij TAB, not per bound agent. The
# unique tab_id count across every pane is what the bar actually renders.
count_live_tabs() {
  local panes
  panes="$(ct_list_panes)" || return 1
  jq '[.[] | .tab_id] | unique | length' <<<"$panes" 2>/dev/null
}

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
    printf '[%s %s] NOTE rung %s width belief: measured=unavailable — seek-trace instrumentation is not in the shipped bar (scripts/seek-trace.sh header); not a check, not a PASS\n' \
      "$CURRENT_PHASE" "$(ts)" "$rung"
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
LIVE_START="$(count_live_tabs)" || LIVE_START=0
[[ "$LIVE_START" =~ ^[0-9]+$ ]] || LIVE_START=0

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

# `add.rs`'s own `zellij action new-tab` call is NOT session-scoped the way
# open.rs's calls are (open.rs pins `ZELLIJ_SESSION_NAME` on every zellij
# invocation it makes, §6.9; `clave add` inherits whatever the caller's
# shell already has) — so this script has to close that gap itself, the
# same belt-and-braces discipline ct.sh applies to every zellij touch
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

print_summary
