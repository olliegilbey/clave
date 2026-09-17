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

# The three readings phase 6c compares across a session boundary. Each takes
# a `dev_status` document as its argument rather than reading one itself, so
# the two sides of the comparison are the SAME snapshot shape and can be
# tested offline (qa/lib-selftest.sh) — this branch's whole lesson is that a
# relaunch assertion nobody can run without a launched session is one nobody
# runs. Sorted, because every use is a set comparison. Empty on an unreadable
# document, never an error: the phase asserts the sets are non-empty itself.

# A row is BOUND when it holds a tab id. The bind is the tab_id and nothing
# else (store.rs §6.6) — a pane with no tab is a row mid-spawn, not a member
# of the live set.
bound_uuids() {
  jq -r '.store.agents | to_entries[] | select(.value.tab_id != null) | .key' <<<"$1" 2>/dev/null | sort
}

# The HELD signature: a tab and no pane. A restored row wears this from the
# moment the bar binds it until its agent is woken, so it is the reading that
# separates a fleet the bar rebound by itself from tabs a human landed on.
held_bound_uuids() {
  jq -r '.store.agents | to_entries[]
         | select(.value.tab_id != null and .value.pane_id == null) | .key' <<<"$1" 2>/dev/null | sort
}

# Which tab phase 6c should close on its way to the quit, so the restore has
# one it must refuse. Takes the `dev status` document and the uuid to SPARE.
# Empty when no row is bound.
#
# The spare is the row the drive MINTED (`clave add`, phase 2 rung 1), and it
# must survive into the restored set. It is the only row in this sandbox whose
# claude gets past the "trust this folder" prompt, so it is the only row that
# ever starts a session, fires a hook, or can reach the #261 `SessionEnd`
# unbind at all. Every seeded row sits at that prompt forever. Close the
# minted row and the phase measures the one population the defect cannot
# touch — which is exactly how the 2026-09-15 verification read green over a
# broken restore.
#
# Two earlier rules failed here and are worth not repeating. "Lowest bound tab
# id" takes the minted row, because it is created first (run 14, 2026-09-16).
# "A tab and no pane" never matches in the FIRST session: that signature
# belongs to a RESTORED row, and every row this drive opens registers a pane
# (run 15, same day — the measure line read `was running`).
#
# Falls back to the spare when it is the only bound row, so a one-row fleet
# still gets the closed-tab half rather than silently skipping it.
close_candidate_tab() {
  jq -r --arg spare "${2:-}" '
    [.store.agents | to_entries[] | select(.value.tab_id != null)] as $bound
    | (([$bound[] | select(.key != $spare) | .value.tab_id] | sort)
       + ([$bound[] | .value.tab_id] | sort))
    | .[0] // empty' <<<"$1" 2>/dev/null
}

# What the PREVIOUS session left. Written at launch, from the binds standing
# when the session died (setup.rs `clear_session_order`), which is also the
# pass that clears them — so this is the only surviving record of the fleet,
# and the expectation the rebound set is measured against.
# Restored rows that still carry a status from the session BEFORE. Read over
# the HELD signature only (a tab, no pane): those rows have run nothing in this
# session, so any status on them is a claim about a process that is gone. The
# eager row is excluded by construction — its agent really did start — so this
# can never go red on legitimate state. (#261)
stale_status_uuids() {
  jq -r '.store.agents | to_entries[]
         | select(.value.tab_id != null and .value.pane_id == null
                  and .value.status != "idle") | .key' <<<"$1" 2>/dev/null | sort
}

last_live_uuids() {
  jq -r '.store.last_live[]?' <<<"$1" 2>/dev/null | sort
}

# How many rows a set holds, and the set on one line for a verdict a human
# reads. Non-empty lines only: two empty sets compare EQUAL, so a set
# comparison built on a dead read reports a perfect restore. The count is
# what the phase refuses on before it compares anything.
uuid_count() { printf '%s' "${1:-}" | grep -c .; }
uuid_line() { printf '%s' "${1:-}" | tr '\n' ' '; }

# The relaunch verdict (phase 6c): does the fleet the second session holds
# match the one the first session left? Takes the set measured before the
# quit, the `dev status` read after the relaunch, and the row whose tab the
# phase closed on its way to the quit. Every reading comes from one snapshot,
# so no two verdicts below can disagree about which moment they are
# describing.
#
# `closed` must be a row that was unbound AT THE QUIT, which is why the phase
# closes its own tab rather than naming one an earlier phase closed: phase 4
# wakes the top wakeable dormant row, and that is exactly the row phase 3
# leaves behind (run 13, 2026-09-16).
#
# Here rather than in the drive because the phase costs two maintainer
# launches, and a verdict that can only be tried by spending them is a verdict
# nobody tries. The selftest runs it against a decayed store, where it must go
# red.
relaunch_checks() {
  local before="$1" status="$2" closed="${3:-}"
  local recorded set_after held n_before n_after n_recorded n_held
  recorded="$(last_live_uuids "$status")"
  set_after="$(bound_uuids "$status")"
  held="$(held_bound_uuids "$status")"
  n_before="$(uuid_count "$before")"
  n_after="$(uuid_count "$set_after")"
  n_recorded="$(uuid_count "$recorded")"
  n_held="$(uuid_count "$held")"
  measure "the set the launch recorded to restore" "n=${n_recorded} $(uuid_line "$recorded")"
  measure "the set the second session rebound" "n=${n_after} $(uuid_line "$set_after")"

  # First, that anything was read at all. Two empty sets compare equal, so
  # every verdict under this one would pass on a dead store.
  check_min "the quit recorded a set to restore" "$n_recorded" 1

  # The first session RECORDED what it was holding. A restore that bakes the
  # right tabs from a set two launches old passes everything below it.
  check "the quit recorded the set the first session was holding" \
    "$(uuid_line "$recorded")" "$(uuid_line "$before")"

  # The decay assertion (#261). The defect bound only the tab the maintainer
  # was looking at, so this number read 1 against a fleet of four.
  check "the live set comes back the SAME SIZE" "$n_after" "$n_before"

  # And it is the same fleet, not the same COUNT of something else.
  check "and holds the same uuids" \
    "$(uuid_line "$set_after")" "$(uuid_line "$recorded")"

  # A tab id with no pane id is the restored leg's signature: the bar bound
  # the row while the tab still held nothing. Every row but the eager one
  # wears it — the eager row spawns at launch — so the floor is one less than
  # the set.
  check_min "restored rows were bound before their agent ran (tab, no pane)" \
    "$n_held" "$((n_before - 1))"

  # A status is scoped to a zellij session, so a restored row must carry none
  # (#261). The branch's `Exited` made the cost visible: a tab about to start
  # its agent came back wearing the hollow "nothing here" mark, beside rows in
  # exactly the same state drawn as live. `Working` had the same shape before
  # it, spinning over a turn that stopped at the quit.
  local stale
  stale="$(stale_status_uuids "$status")"
  check "restored rows carry no status from the session before" \
    "$(uuid_line "$stale")" ""

  # A closed tab stays closed. Not covered by the three above: a row still
  # bound when its tab went (a prune that did not happen, phase 3's family)
  # is in every set, so all of them match and the fleet still comes back one
  # tab too wide.
  if [[ -n "$closed" ]]; then
    check "the tab closed in the first session is absent from the second" \
      "$(grep -c -- "$closed" <<<"$set_after")" "0"
  fi
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
  # A caller that opened no phase still gets a FAILING exit. Without this the
  # arithmetic below is `PHASE_RESULTS[-1]`, which aborts the function under
  # `set -u` with `bad array subscript` — killing the `exit 1` two lines down
  # and returning 0. Measured in review: `relaunch-verdict.sh` printed a red
  # verdict and exited 0, and it is the documented recovery for a timed-out
  # phase 6c. A check that cannot fail the run is not a check.
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
