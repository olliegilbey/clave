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

# LINES matching <pattern> since <mark>, from this sandbox's instances only —
# the per-line count the per-instance forms above deliberately are not. A
# flap is one instance logging the same ask sixteen times (the devbox,
# 2026-09-22), and an instance count reads that as 1. Attributed the same
# way: the line's `[id: N]` must belong to an instance that announced this
# build.
sandbox_lines_since() {
  local mark="$1" pattern="$2" ids
  ids="$(sandbox_instance_ids | tr '\n' ' ')"
  zlog_from "$mark" | grep -F "$pattern" | while IFS= read -r line; do
    id="$(printf '%s\n' "$line" | log_ids)"
    [[ -n "$id" && " $ids " == *" $id "* ]] && printf '%s\n' "$line"
  done
}

sandbox_line_count_since() {
  sandbox_lines_since "$1" "$2" | grep -c . || true
}

# The width asks (`clave-bar: swap-width …`, one line per ask the shell
# forwards to zellij) this sandbox's bars made since <mark>. A bar at its
# declared width asks nothing, so outside a toggle this is a defect count.
swap_ask_count_since() {
  sandbox_line_count_since "$1" 'clave-bar: swap-width'
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

# The readings phase 6c takes on both sides of a session boundary. Each takes
# a `dev_status` document as its argument rather than reading one itself, so
# the two sides are the SAME snapshot shape and can be tested offline
# (qa/lib-selftest.sh): a relaunch assertion nobody can run without a
# launched session is one nobody runs. Sorted, because every use is a set
# comparison. Empty on an unreadable document, never an error: the verdict
# refuses on the count itself.

# A row is BOUND when it holds a tab id. The bind is the tab_id and nothing
# else (store.rs §6.6) — a pane with no tab is a row mid-spawn, not a bound
# row.
bound_uuids() {
  jq -r '.store.agents | to_entries[] | select(.value.tab_id != null) | .key' <<<"$1" 2>/dev/null | sort
}

# The row a relaunch bakes its ONE tab for: the most-recent row by
# `last_interacted` whose cwd is still a directory (setup.rs `eager_row`).
# Read from the PRE-QUIT snapshot, because the launch computes it from the
# store the quit left. The cwd test is done here, not in jq, because jq
# cannot stat a path; the drive runs on the machine the sandbox runs on, so
# the test sees the same disk the launch does. Empty when no row qualifies,
# which is the bar-only layout.
eager_candidate_uuid() {
  local uuid cwd
  while IFS=$'\t' read -r uuid cwd; do
    [[ -n "$uuid" && -d "$cwd" ]] && { printf '%s\n' "$uuid"; return 0; }
  done < <(jq -r '.store.agents | to_entries
                  | sort_by(-.value.last_interacted)[]
                  | [.key, .value.cwd] | @tsv' <<<"$1" 2>/dev/null)
  return 0
}

# Rows wearing a status that describes a RUNNING process. A launch starts no
# agent and clears Working and NeedsYou on the same pass that clears the binds
# (store.rs `clear_session_order`), and the drive types nothing between the
# relaunch and this read — so any row carrying one after the relaunch is a
# claim about a process that is gone (#261, QA run 18). `Done` and `Failed`
# survive by design: they mark a finished turn the human has not read.
stale_status_uuids() {
  jq -r '.store.agents | to_entries[]
         | select(.value.status == "working" or .value.status == "needs_you") | .key' <<<"$1" 2>/dev/null | sort
}

# Rows on standby: a quit stamped them (hook.rs, SessionEnd with reason
# `other`, or the launch for a row still bound, store.rs
# `clear_session_order`), and no bind, prune or expiry has spent the stamp
# since. The store record carries the stamp; the wire carries its time.
standby_uuids() {
  jq -r '.store.agents | to_entries[] | select(.value.standby_stamp != null) | .key' <<<"$1" 2>/dev/null | sort
}

# The rows a relaunch must bring back on standby. Read from the PRE-QUIT
# snapshot, like the eager row: every row bound to a tab (its agent is running,
# so the quit's SessionEnd or the launch stamps it) and every row already stamped, minus the
# eager row, whose bind at the launch spends its stamp. $2 is that eager row.
standby_expected_uuids() {
  jq -r --arg eager "${2:-}" '.store.agents | to_entries[]
         | select((.value.tab_id != null or .value.standby_stamp != null) and .key != $eager)
         | .key' <<<"$1" 2>/dev/null | sort
}

# How many rows a set holds, and the set on one line for a verdict a human
# reads. Non-empty lines only: two empty sets compare EQUAL, so a set
# comparison built on a dead read reports a perfect relaunch. The count is
# what the verdict refuses on before it compares anything.
uuid_count() { printf '%s' "${1:-}" | grep -c .; }
uuid_line() { printf '%s' "${1:-}" | tr '\n' ' '; }

# The relaunch verdict (phase 6c). A relaunch bakes ONE tab, for the row
# `eager_candidate_uuid` names. The rows the quit left live come back on
# standby, every other row dormant, and none with a running-process status (setup.rs `launch_layout_kdl`, decision of
# 2026-09-22). Takes the uuid expected to be bound, computed from the
# pre-quit snapshot, and the `dev status` read after the relaunch. Every
# reading below comes from that one snapshot, so no two checks can disagree
# about which moment they describe.
#
# Here rather than in the drive because the phase costs two maintainer
# launches, and a verdict that can only be tried by spending them is a verdict
# nobody tries. The selftest runs it against a store with two bound rows, the
# wrong row bound, a stale status, and an empty read — each must go red.
relaunch_checks() {
  local expected="$1" status="$2" expected_standby="${3:-}"
  local set_after n_after stale
  set_after="$(bound_uuids "$status")"
  n_after="$(uuid_count "$set_after")"
  measure "the row the launch was expected to bake" "$expected"
  measure "the set the second session bound" "n=${n_after} $(uuid_line "$set_after")"

  # First, that the expectation exists. An empty expected uuid against an
  # empty bound set would compare equal, and a dead pre-quit read would then
  # pass as a perfect relaunch.
  check_nonempty "the pre-quit snapshot named a row to bake" "$expected"

  # Exactly one tab. Zero is a launch that baked nothing for a fleet it had;
  # two or more is a fleet in one layout, the shape that killed the zellij
  # server (#261, measured 2026-09-17).
  check "the relaunch bound exactly ONE row" "$n_after" "1"

  # And it is the most-recent row, not whichever the launch happened to pick.
  check "and that row is the most-recent one" "$(uuid_line "$set_after")" "$expected"

  # Every other row came back unbound, and none wears a status from the
  # session before.
  stale="$(stale_status_uuids "$status")"
  check "no row carries a running-process status from the session before" \
    "$(uuid_line "$stale")" ""

  # The rows the quit left live come back on standby (Ollie, 2026-09-22).
  # Refused when the expectation is empty, because two empty sets compare
  # equal: a quit that stamped nothing would pass against a one-row fleet.
  # A row missing here was lost in the quit: SessionEnd did not say `other`,
  # or a bar pruned its tab while the session went down.
  check_nonempty "the pre-quit snapshot named rows the quit leaves on standby" \
    "$(uuid_line "$expected_standby")"
  check "every row the quit left live is on standby" \
    "$(uuid_line "$(standby_uuids "$status")")" "$(uuid_line "$expected_standby")"
}

# The arrival verdict (phase 6c, after the beacon leg). One Alt+Down from the
# baked tab lands on the top standby row, and the landing opens it with no
# Alt+Enter (Ollie, 2026-09-22). $1 is the baked row, $2 the standby set
# before the press, $3 the `dev status` read after it. The opened row is the
# one bound row that is not the baked one. Its bind must spend its stamp.
arrival_checks() {
  local eager="$1" standby_before="$2" status="$3"
  local opened was_standby="no" stamp="unread"
  opened="$(bound_uuids "$status" | grep -vx -- "$eager" || true)"
  measure "the row one Alt+Down opened" "$(uuid_line "$opened")"
  check "one Alt+Down opened exactly one row" "$(uuid_count "$opened")" "1"
  if [[ -n "$opened" ]] && grep -qx -- "$opened" <<<"$standby_before"; then
    was_standby="yes"
  fi
  check "and that row was on standby (a dormant row waits for Alt+Enter)" "$was_standby" "yes"
  [[ -n "$opened" ]] && stamp="$(jq -r --arg u "$opened" '.store.agents[$u].standby_stamp' <<<"$status" 2>/dev/null)"
  check "and its bind spent the stamp" "$stamp" "null"
  # A walk is not a commitment: the new tab ranks by the row's own ordinal,
  # never a fresh top one (Ollie, 2026-09-23; store.rs `apply_bind`).
  local rank="unread"
  [[ -n "$opened" ]] && rank="$(jq -r --arg u "$opened" '
    .store as $s | $s.agents[$u] as $a
    | ($s.tab_order // {})[($a.tab_id | tostring)] as $t
    | if $a.commit_ord == null then "unread"
      elif $t == $a.commit_ord then "held"
      else "tab \($t) over row \($a.commit_ord)" end' <<<"$status" 2>/dev/null)"
  check "and its tab kept the row's own rank" "$rank" "held"
}

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
# The join assumes the dump lists tabs in tab position order. SETTLED
# (runs 24 and 25, 2026-09-22): phase 6c compares this id against the
# `tab=Some(N)` the bar computes for itself from its own frames, on both
# hosts, and they agree. Every earlier caller anchored to whatever it read, so
# a wrong join passed self-consistently; 6c is the first cross-check. If 6c's
# `tab=Some(N)` line goes red while its ask count is green, suspect this join
# before the bar.
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
  # A caller that opened no phase still gets a FAILING exit. Without this the
  # arithmetic below is `PHASE_RESULTS[-1]`, which aborts the function under
  # `set -u` with `bad array subscript` — killing the `exit 1` two lines down
  # and returning 0. Measured in review (2026-09-17): a standalone verdict
  # tool that opened no phase printed a red verdict and exited 0. The tool is
  # gone; the guard stays, because a check that cannot fail the run is not a
  # check.
  if (( ${#PHASE_RESULTS[@]} == 0 )); then
    printf '\nFAILED (no phase open)\n'
    exit 1
  fi
  local last=$((${#PHASE_RESULTS[@]} - 1))
  PHASE_RESULTS[last]="FAIL"
  printf '\nPHASE %s FAILED\n' "$CURRENT_PHASE"
  print_summary
  exit 1
}

# Mark the current phase NOT RUN and carry on. A phase that could not run is
# not a phase that failed: it measured nothing, so it has no verdict to give,
# and calling it red hides which phases are actually red.
#
# For the case where the run did not get a PRECONDITION it cannot supply
# itself — phase 6c waits on a maintainer, and the first live run (2026-09-16)
# timed out and took ten green phases down with it. Never for a reading that
# came back wrong; that is `check`'s job and it stops the run.
skip_phase() {
  local why="$1"
  if (( ${#PHASE_RESULTS[@]} == 0 )); then
    printf 'NOT RUN: %s\n' "$why"
    return
  fi
  local last=$((${#PHASE_RESULTS[@]} - 1))
  PHASE_RESULTS[last]="NOT RUN"
  printf '[%s %s] NOT RUN: %s\n' "$CURRENT_PHASE" "$(ts)" "$why"
}

# The drive's last act. A run that skipped a phase is not a green run, and the
# exit code is what a release runbook, an agent, or a person scrolling back
# actually reads. The summary prints first — those readings are still worth
# having — and then the status tells the truth about what is missing.
exit_on_incomplete() {
  local i missing=()
  for i in "${!PHASE_RESULTS[@]}"; do
    [[ "${PHASE_RESULTS[$i]}" == "NOT RUN" ]] && missing+=("${PHASE_NAMES[$i]}")
  done
  ((${#missing[@]} == 0)) && return 0
  printf '\nINCOMPLETE: %s did not run. The drive is not green until it does.\n' "${missing[*]}"
  exit 1
}
