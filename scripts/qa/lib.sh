#!/usr/bin/env bash
# scripts/qa/lib.sh — the QA instrument: tracing, assertions, and the zellij
# log's per-instance readings. Sourced, never executed.
#
# Why it is a file of its own: `qa-drive.sh` is the DRIVE — a sequence of
# phases that presses things and reads what happened. Everything here is the
# INSTRUMENT it reads with, and an instrument is worth testing on its own
# (`scripts/qa/lib-selftest.sh`, gated by `crates/clave/tests/qa_lib.rs`).
# Before the split, the log parsing lived inline among 2400 lines of drive and
# could only be exercised by asking the maintainer to launch a session.
#
# The caller must set, before sourcing:
#   SCENARIO   the scenario name, for the summary header
#   DRIVE_LOG  the tee'd log path, for the summary footer
#   ZLOG       zellij's user-global log file
#   LOGMARK    lines already in ZLOG when the run started (0 if unreadable)
#   BUILD_TAG  the short sha `just sandbox` baked into this sandbox's wasm
#   CLAVE_BIN  the sandbox-aware `clave` under test
#   CT         scripts/ct.sh, the only sanctioned zellij wrapper
#
# Everything the drive asserts about the bar comes through here, because the
# bar has no other observable: its pane is invisible to `list-panes` and
# `dump-screen` is empty for plugin panes (FOOTGUNS). Its own log lines ARE
# the instrumentation.

# ---------------------------------------------------------------------------
# phase()/check()/measure() — the tracing spec's helpers (QA-DRIVE.md).
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

# Something the reader has to know that is neither a reading nor a verdict: a
# skip, a precondition this run did not get, an interpretation. Every phase
# grew its own `printf '[%s %s] NOTE …'`; this is that line, once. Takes a
# printf format so every one of those sites converts without rewriting its
# substitutions.
note() {
  local fmt="$1"
  shift
  # shellcheck disable=SC2059 # the format IS the argument here, by design
  printf "[%s %s] NOTE $fmt\n" "$CURRENT_PHASE" "$(ts)" "$@"
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

# ---------------------------------------------------------------------------
# The zellij log, read per BAR INSTANCE.
# ---------------------------------------------------------------------------
# zellij stamps every plugin line with the instance that wrote it:
#
#   DEBUG |/Users/x/.local| 2026-09-12 14:10:48.319 [id: 6     ] clave-bar: …
#
# That `[id: N]` is the only handle the drive has on an individual bar, and
# until 2026-09-12 nothing read it — every count was a count of LINES, which
# cannot tell one busy instance from five quiet ones. The whole fleet's
# behaviour looked identical to one instance's. The terminal-facts flicker was
# invisible for exactly that reason: one instance of five had the facts, and
# the line count said "the pipeline delivered".

# Everything phase 1+ reads from the zellij log is lines AFTER the mark the
# drive took at its start. The build-tag readings below are the deliberate
# exceptions, and say so.
#
# A phase that presses something and wants to read only what IT caused takes
# its own sub-mark with `zlog_now` and reads with the `_since` forms: the
# run-long mark cannot tell a line this leg produced from one an earlier phase
# did, and a witness that counts history is not a witness.
zlog_now() {
  if [[ -r "$ZLOG" ]]; then
    wc -l <"$ZLOG" | tr -d ' '
  else
    echo 0
  fi
}

zlog_from() {
  local mark="$1"
  if [[ -r "$ZLOG" ]]; then
    tail -n "+$((mark + 1))" "$ZLOG"
  fi
}

zlog_tail() { zlog_from "$LOGMARK"; }

# Distinct `[id: N]` values on stdin, sorted. Lines without the stamp are
# ignored rather than counted as an unnamed instance.
log_ids() {
  sed -n 's/.*\[id: *\([0-9][0-9]*\) *\].*/\1/p' | sort -u
}

# Every bar instance belonging to THIS sandbox, by build tag.
#
# Deliberately UNMARKED — it reads the whole log, not the tail after the mark,
# for the same reason phase 0's build-tag check does: the bars the human's
# launch created loaded BEFORE this script started, so a marked read sees only
# the instances born during the drive and would leave the launch fleet out of
# every denominator.
#
# Attributable by CONTENT, with the residual the tag always carries: a second
# worktree sitting at the same HEAD shares it, and a previous run of the same
# build in the same session contributes its ids too (the same instances, in
# practice, since ids are per-server).
sandbox_instance_ids() {
  if [[ -r "$ZLOG" ]]; then
    grep -F 'clave-bar: loaded' "$ZLOG" | grep -F "build=$BUILD_TAG" | log_ids
  fi
}

sandbox_instance_count() {
  sandbox_instance_ids | grep -c . || true
}

# Fresh `clave-bar: loaded` lines carrying THIS build's tag, since the script's
# log mark. The only way to count bar instances at all: the bar is invisible to
# `list-panes` (FOOTGUNS, "list-panes does not show the clave-bar at all"), so
# a new tab's bar is proved to have loaded by its own log line and nothing
# else.
bar_loaded_count() {
  local n
  n="$(zlog_tail | grep -F 'clave-bar: loaded' | grep -c -F "build=$BUILD_TAG")" || n=0
  printf '%s' "${n:-0}"
}

# The sandbox instances that logged <pattern> since the mark, as a sorted list.
#
# The intersection is what makes this attributable: the log is user-global and
# most bar lines carry no build tag, so ids are crossed with the instances that
# announced THIS build. Without that, the maintainer's live fleet answers for
# our sandbox.
instances_logging_since() {
  local mark="$1" pattern="$2"
  comm -12 <(zlog_from "$mark" | grep -F "$pattern" | log_ids) <(sandbox_instance_ids)
}

instances_logging() { instances_logging_since "$LOGMARK" "$1"; }

instance_count_logging_since() {
  instances_logging_since "$1" "$2" | grep -c . || true
}

instance_count_logging() {
  instances_logging "$1" | grep -c . || true
}

# ---------------------------------------------------------------------------
# The session readers: the store, the layout, and focus.
# ---------------------------------------------------------------------------
# Every one of these goes through `scripts/ct.sh`, which refuses closed when
# the sandbox session is not live rather than falling back to whatever session
# the caller's shell sits inside. They are here rather than in the drive so a
# one-off diagnostic can read a sandbox with the same guards the drive uses —
# a hand-rolled `zellij action` is how a drive ends up in the maintainer's
# fleet.
#
# Additionally expects set: CLAVE_BIN, CT.

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

ct_dump_layout() {
  local out
  if ! out="$("$CT" dump-layout)"; then
    printf '[%s %s] ct.sh dump-layout FAILED (stderr above)\n' "$CURRENT_PHASE" "$(ts)" >&2
    return 1
  fi
  printf '%s' "$out"
}

# The live tab id set as a compact JSON array. One bar per tab is the layout's
# design, so this is also the bar's LIVE BLOCK membership (count_live_tabs'
# rationale, phase 2).
live_tab_ids() {
  local panes
  panes="$(ct_list_panes)" || return 1
  jq -c '[.[] | .tab_id] | unique' <<<"$panes" 2>/dev/null
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
    note 'go-to-tab-by-id refused for tab %s — falling back to positional go-to-tab' "$want"
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

# The per-instance ledger: for every sandbox bar, which capabilities its own
# log shows it exercising during the run. Printed at the end of every drive,
# asserted nowhere — it is the reading a human wants FIRST when a phase goes
# red, and the shape that tells a fleet-wide behaviour from a single
# instance's.
#
# Capabilities are (label, log fragment) pairs. Each one is a line the bar
# emits ONLY on a state change (main.rs, "CHANGED-ONLY logging"), so a blank
# cell means "never happened here", not "not instrumented".
QA_CAPABILITIES=(
  "term-facts:clave-bar: term-facts probe updated"
  "cwd-delta:clave-bar: term-facts cwd delta"
  "cmd-delta:clave-bar: term-facts command delta"
  "dropped:clave-bar: dropped"
)

instance_ledger() {
  local ids id cap label frag marks
  ids="$(sandbox_instance_ids)"
  if [[ -z "$ids" ]]; then
    echo "  (no build-tagged bar instances in the zellij log — nothing to ledger)"
    return
  fi
  printf '  %-10s %s\n' "instance" "capabilities seen since the log mark"
  while read -r id; do
    [[ -z "$id" ]] && continue
    marks=""
    for cap in "${QA_CAPABILITIES[@]}"; do
      label="${cap%%:*}"
      frag="${cap#*:}"
      if zlog_tail | grep -F "$frag" | grep -qF "[id: $id"; then
        marks+="${label} "
      fi
    done
    printf '  %-10s %s\n' "id $id" "${marks:-—}"
  done <<<"$ids"
}

print_summary() {
  echo
  echo "== QA drive summary (${SCENARIO}) =="
  local i
  for i in "${!PHASE_NAMES[@]}"; do
    printf '  %-18s %s\n' "${PHASE_NAMES[$i]}" "${PHASE_RESULTS[$i]}"
  done
  echo
  echo "== bar instances (zellij log, build=${BUILD_TAG}) =="
  instance_ledger
  echo
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
