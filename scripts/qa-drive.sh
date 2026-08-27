#!/usr/bin/env bash
# qa-drive.sh — the automated regression drive, phases 0-7 (docs/dev/QA-DRIVE.md).
#
# What this is: preflight, baseline join, the dormant-row bind ladder, tab
# churn, the nav ring walk, the collapse burst, quiescence and the teardown
# hand-back, scripted against THIS checkout's per-worktree sandbox instance.
#
# ALL PHASES DRIVEN LIVE GREEN — run 4, 2026-08-17, full 0-7 pass plus both
# human eyeball checkpoints. The list below was the FIRST LIVE RUN PENDING
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
ring walk, collapse burst, quiescence, teardown hand-back) against
THIS checkout's per-worktree sandbox instance (\`clave dev instance\`). Never
launches or kills a zellij session — stage and launch first:

  just sandbox <scenario>
  clave dev scenario <scenario>   # if not already seeded by \`just sandbox\`
  (human, non-zellij terminal) clave dev launch

then run this. Refuses closed if the instance's sandbox session is not live.

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
  echo "Full 0-7 driven live green: run 4, 2026-08-17 — the header's ledger records how each pending assumption settled."
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

# check_numeric <desc> <measured> — measured must be a bare integer. The
# guard for asserted READS: a `// 0` or `:-0` fallback lets a dead
# dev_status or an unreadable log read 0 on BOTH ends of a window, and a
# flat/bounded check then passes without having observed anything
# (CodeRabbit, PR #202 — same family as run 3's jq `//` blindness to
# `false`). jq prints `null` for a missing key, which this rejects too.
check_numeric() {
  local desc="$1" measured="${2:-}" verdict="FAIL"
  [[ "$measured" =~ ^[0-9]+$ ]] && verdict="PASS"
  printf '[%s %s] CHECK %s: measured=%s expected=<integer> %s\n' "$CURRENT_PHASE" "$(ts)" "$desc" "${measured:-empty}" "$verdict"
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

# Fresh `clave-bar: loaded` lines carrying THIS build's tag, since the script's
# log mark. The only way to count bar instances at all: the bar is invisible to
# `list-panes` (FOOTGUNS, "list-panes does not show the clave-bar at all"), so
# a new tab's bar is proved to have loaded by its own log line and nothing
# else. Attributable by CONTENT (the build tag), which is what makes it usable
# in a user-global log — with one honest residual: a second worktree sitting at
# the same HEAD would share the tag.
bar_loaded_count() {
  local n
  n="$(zlog_tail | grep -F 'clave-bar: loaded' | grep -c -F "build=$BUILD_TAG")" || n=0
  printf '%s' "${n:-0}"
}

# The live tab id set as a compact JSON array. One bar per tab is the layout's
# design, so this is also the bar's LIVE BLOCK membership (count_live_tabs'
# rationale, phase 2).
live_tab_ids() {
  local panes
  panes="$(ct_list_panes)" || return 1
  jq -c '[.[] | .tab_id] | unique' <<<"$panes" 2>/dev/null
}

ct_dump_layout() {
  local out
  if ! out="$("$CT" dump-layout)"; then
    printf '[%s %s] ct.sh dump-layout FAILED (stderr above)\n' "$CURRENT_PHASE" "$(ts)" >&2
    return 1
  fi
  printf '%s' "$out"
}

# WHICH TAB IS FOCUSED, as a tab_id — the one focus observable this drive has,
# and the spine of both phases below.
#
# `list-panes` cannot answer it: `PaneInfo.is_focused` is "focused in its
# LAYER" (zellij-utils data.rs:2302), so every tab reports a focused pane.
# `dump-layout` can: `serialize_tab` writes `focus=true` on the focused tab
# node and on no other (zellij-utils session_serialization.rs:109, snapshot
# `can_serialize_tab_focus`). The dump names no ids, so the focused node's RANK
# among tab nodes is joined back to a tab_id through `list-panes`' own
# tab_position ordering — a rank join, deliberately, because it does not care
# whether zellij counts tab positions from 0 or from 1.
#
# FIRST LIVE RUN PENDING (2): the join assumes the dump lists tabs in tab
# position order. Every caller prints the id it read, so a wrong join shows up
# as a focus that never matches anything rather than as a silent pass.
focused_tab_id() {
  local dump idx panes
  dump="$(ct_dump_layout)" || return 1
  idx="$(awk '$1 == "tab" { i++; if ($0 ~ /focus=true/) { print i; exit } }' <<<"$dump")"
  [[ -z "$idx" ]] && return 1
  panes="$(ct_list_panes)" || return 1
  jq -r --argjson i "$idx" \
    '[.[] | {tab_id, tab_position}] | unique_by(.tab_id) | sort_by(.tab_position) | .[$i - 1].tab_id // empty' \
    <<<"$panes" 2>/dev/null
}

# Focus a tab BY ID, then PROVE it landed. Nothing here touches a pane.
# `go-to-tab-by-id` is zellij 0.44's stable-id action (zellij-utils
# cli.rs:1213 "Go to tab with stable ID"); the positional `go-to-tab` fallback
# is for an older server, and the +1 is the documented 0-indexed tab_position
# → 1-based tab index conversion (data.rs:2277).
#
# FIRST LIVE RUN PENDING (1): which of the two legs the maintainer's server
# takes. The verification loop below is why it does not matter — a fallback
# that converts wrongly fails here, loudly, instead of drifting one tab off.
focus_tab() {
  local want="$1" panes pos got
  if ! "$CT" go-to-tab-by-id "$want"; then
    printf '[%s %s] NOTE go-to-tab-by-id refused for tab %s — falling back to positional go-to-tab\n' \
      "$CURRENT_PHASE" "$(ts)" "$want"
    panes="$(ct_list_panes)" || return 1
    pos="$(jq -r --argjson t "$want" '[.[] | select(.tab_id == $t) | .tab_position] | unique | .[0] // empty' <<<"$panes" 2>/dev/null)"
    [[ -z "$pos" ]] && return 1
    "$CT" go-to-tab "$((pos + 1))" || return 1
  fi
  for _ in $(seq 1 5); do
    got="$(focused_tab_id)"
    [[ -n "$got" && "$got" == "$want" ]] && return 0
    sleep 1
  done
  return 1
}

# Focus a tab and assert the landing, in one line of drive.
focus_tab_checked() {
  local want="$1" label="$2" rc
  focus_tab "$want"
  rc=$?
  check "$label focus landed on tab $want" \
    "$([[ $rc -eq 0 ]] && echo "tab=$want" || echo "focused=$(focused_tab_id)")" "tab=$want"
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
    printf '[%s %s] NOTE %s nav focus check NOT EXERCISED: every tab tried is itself the top live row, so a press could not move focus. Not a pass.\n' \
      "$CURRENT_PHASE" "$(ts)" "$label"
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
  printf '[%s %s] NOTE churn B: the new tab is id %s, not the closed %s — this run did NOT exercise recycling, so the inherited-stamp check below is a control, not the property.\n' \
    "$CURRENT_PHASE" "$(ts)" "$NEW_TAB" "$B_TAB"
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
  printf '[%s %s] NOTE churn B: the new tab has NO tab_order stamp. That is the birth_touch latch signature (B15/#55) — record it, do not read it as this phase failing.\n' \
    "$CURRENT_PHASE" "$(ts)"
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
# Phase 7 — teardown (the hand-back)
# ===========================================================================
# Asserts nothing, launches nothing, kills nothing: session lifecycle is the
# human's (AGENTS.md, TESTING.md "the interaction contract"). The drive's
# last act is to print the kill pair and the two eyeball checkpoints it owes.
phase "P7-teardown"

measure "sandbox left as driven; store, evlog and drive log preserved for forensics" "$STATE_DIR"
cat <<EOF

The two eyeball checkpoints (human, one message each — QA-DRIVE):
  1. one bar per tab; woken rows show agent chips, not terminal glyphs
  2. every tab a strip (or every tab wide) — no width outliers

Teardown, when done (human, non-zellij terminal):
  zellij kill-session ${SESSION}
  zellij delete-session --force ${SESSION}
EOF

print_summary
