#!/usr/bin/env bash
# scripts/qa/lib-selftest.sh — the instrument, tested against a fixture log.
#
# The drive itself needs a maintainer-launched zellij session, so for a year
# the only way to find out whether its log parsing was right was to ask him to
# launch one. That is the wrong loop for a `sed` expression. Everything here
# runs offline in under a second, and `crates/clave/tests/qa_lib.rs` runs it
# in the normal `cargo test` gate.
#
# Scope: the readings, not the drive. A phase that presses zellij belongs in
# qa-drive.sh; a function that turns log text into a number belongs here.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

FAILURES=0
want() {
  local desc="$1" got="$2" expected="$3"
  if [[ "$got" == "$expected" ]]; then
    printf 'ok   %s\n' "$desc"
  else
    printf 'FAIL %s\n       got:      %s\n       expected: %s\n' "$desc" "${got:-<empty>}" "${expected:-<empty>}"
    FAILURES=$((FAILURES + 1))
  fi
}

# The fixture: one zellij log holding two fleets, which is the shape the real
# one always has — the maintainer's live session and this sandbox write to the
# same user-global file, and only the build tag on a `loaded` line tells them
# apart. Instance ids COLLIDE across fleets (id 3 is both), deliberately:
# that collision is the residual the lib documents, and a parser that ignores
# the tag reads his bars as ours.
# `mktemp -t <name>` is NOT portable: GNU mktemp treats the argument as a
# template and refuses one with no `X`s, while BSD/macOS takes it as a prefix.
# A full template works on both (CI caught this — it greens on the maintainer's
# mac and dies on ubuntu).
FIXTURE="$(mktemp "${TMPDIR:-/tmp}/qa-lib-selftest.XXXXXX")"
if [[ -z "$FIXTURE" || ! -f "$FIXTURE" ]]; then
  echo "FAIL cannot create the fixture log — every reading below would be vacuous" >&2
  exit 1
fi
trap 'rm -f "$FIXTURE"' EXIT
cat >"$FIXTURE" <<'LOG'
DEBUG  |/Users/x/.local| 2026-09-12 13:44:54.761 [id: 2     ] clave-bar: loaded v0.4.0 build=deadbee
DEBUG  |/Users/x/.local| 2026-09-12 13:44:55.001 [id: 3     ] clave-bar: loaded v0.4.0 build=deadbee
DEBUG  |/Users/x/.local| 2026-09-12 13:45:10.810 [id: 3     ] clave-bar: loaded v0.4.0 build=cafe123
DEBUG  |/Users/x/.local| 2026-09-12 13:45:21.612 [id: 4     ] clave-bar: loaded v0.4.0 build=cafe123
DEBUG  |/Users/x/.local| 2026-09-12 13:45:22.100 [id: 6     ] clave-bar: loaded v0.4.0 build=cafe123
DEBUG  |/Users/x/.local| 2026-09-12 13:46:30.640 [id: 5     ] clave-bar: loaded v0.4.0 build=cafe123
INFO   |zellij_server  | 2026-09-12 13:47:00.000 a line with no instance stamp at all
DEBUG  |/Users/x/.local| 2026-09-12 13:47:01.000 [id: 2     ] clave-bar: term-facts probe updated
DEBUG  |/Users/x/.local| 2026-09-12 13:47:02.000 [id: 3     ] clave-bar: term-facts probe updated
DEBUG  |/Users/x/.local| 2026-09-12 13:47:02.100 [id: 3     ] clave-bar: swap-width backwards=false cols=47
DEBUG  |/Users/x/.local| 2026-09-12 13:47:02.200 [id: 3     ] clave-bar: swap-width backwards=false cols=47
DEBUG  |/Users/x/.local| 2026-09-12 13:47:02.300 [id: 2     ] clave-bar: swap-width backwards=false cols=47
DEBUG  |/Users/x/.local| 2026-09-12 13:47:03.000 [id: 4     ] clave-bar: term-facts command delta pane 7
DEBUG  |/Users/x/.local| 2026-09-12 13:47:04.000 [id: 5     ] clave-bar: dropped an event at EOF
LOG
FIXTURE_LINES="$(wc -l <"$FIXTURE" | tr -d ' ')"

# What the lib expects the caller to have set.
ZLOG="$FIXTURE"
LOGMARK=0
BUILD_TAG="cafe123"
SCENARIO="selftest"
DRIVE_LOG="/dev/null"
# shellcheck source=scripts/qa/lib.sh
source "$SCRIPT_DIR/lib.sh"

# --- the log readers -------------------------------------------------------

want "log_ids takes every distinct stamp, sorted, and ignores unstamped lines" \
  "$(log_ids <"$FIXTURE" | tr '\n' ' ')" "2 3 4 5 6 "

want "sandbox_instance_ids keeps only instances that announced THIS build" \
  "$(sandbox_instance_ids | tr '\n' ' ')" "3 4 5 6 "

want "sandbox_instance_count counts them" "$(sandbox_instance_count)" "4"

# The intersection is the whole point: id 2 wrote a term-facts line but never
# loaded this build, so it is the maintainer's bar and must not be counted.
want "instances_logging attributes a tagless line by intersecting the fleet" \
  "$(instances_logging 'clave-bar: term-facts probe updated' | tr '\n' ' ')" "3 "

want "instance_count_logging counts the attributed set" \
  "$(instance_count_logging 'clave-bar: term-facts')" "2"

want "a pattern nothing logged reads zero, not empty" \
  "$(instance_count_logging 'clave-bar: never-happened')" "0"

# A sub-mark is how a phase reads only what its own leg caused.
want "instances_logging_since ignores everything before the mark" \
  "$(instance_count_logging_since "$((FIXTURE_LINES - 1))" 'clave-bar: term-facts')" "0"

want "instances_logging_since sees a line after the mark" \
  "$(instance_count_logging_since "$((FIXTURE_LINES - 2))" 'clave-bar: term-facts')" "1"

want "zlog_now counts the lines in the log" "$(zlog_now)" "$FIXTURE_LINES"

# The flap's signature is one instance asking many times. The per-instance
# reading says 1; only a per-LINE reading says 2 — and the maintainer's
# instance 2 asked too, and must not be counted.
want "instances_logging reads a flap as one instance" \
  "$(instance_count_logging 'clave-bar: swap-width')" "1"
want "swap_ask_count_since counts every ask our instances made" \
  "$(swap_ask_count_since 0)" "2"
want "swap_ask_count_since leaves the maintainer's ask out" \
  "$(sandbox_lines_since 0 'clave-bar: swap-width' | grep -c 'id: 2')" "0"
want "swap_ask_count_since honours the mark" \
  "$(swap_ask_count_since "$((FIXTURE_LINES - 2))")" "0"

# An unreadable log must read as nothing, never as an error or a hang: the
# drive runs before any bar has logged, and on a machine where zellij's log
# has been rotated away.
want "a missing log reads as zero instances" \
  "$(ZLOG="/nonexistent/zellij.log" sandbox_instance_count)" "0"
want "a missing log reads as zero lines" \
  "$(ZLOG="/nonexistent/zellij.log" zlog_now)" "0"

# --- the store readers (phase 6c, the relaunch) ----------------------------
# The relaunch phase reads the store on both sides of a session boundary, and
# the only reason it can be trusted is that each reading is taken the same
# way on both sides. The fixture holds one row of every shape the verdict must
# tell apart, because a reader that conflates two of them reports a relaunch
# that did not happen.
#
# The cwd test in `eager_candidate_uuid` needs real directories: one that
# exists for the rows the launch may bake, and one that does not for the row
# it must skip.
CWD_DIR="$(mktemp -d "${TMPDIR:-/tmp}/qa-selftest-cwd.XXXXXX")"
trap 'rm -f "$FIXTURE"; rm -rf "$CWD_DIR"' EXIT
STATUS_FIXTURE="$(jq -nc --arg d "$CWD_DIR" '{
  session: "clave-test-selftest",
  session_live: true,
  store: {
    seq: 41,
    agents: {
      "u-eager":   {uuid: "u-eager",   tab_id: 1,    pane_id: 5,    status: "idle",    last_interacted: 500, cwd: $d},
      "u-ghost":   {uuid: "u-ghost",   tab_id: null, pane_id: null, status: "idle",    last_interacted: 900, cwd: ($d + "/gone")},
      "u-dormant": {uuid: "u-dormant", tab_id: null, pane_id: null, status: "done",    last_interacted: 100, cwd: $d},
      "u-stale":   {uuid: "u-stale",   tab_id: null, pane_id: null, status: "working", last_interacted: 200, cwd: $d},
      "u-stray":   {uuid: "u-stray",   tab_id: null, pane_id: 9,    status: "idle",    last_interacted: 300, cwd: $d}
    }
  }
}')"

want "bound_uuids takes every row with a tab, sorted" \
  "$(bound_uuids "$STATUS_FIXTURE" | tr '\n' ' ')" "u-eager "

# A pane and no tab is not a bind. The bind IS the tab_id (store.rs §6.6), and
# a reader that accepted either field would count a stray pane as a second
# baked tab and fail a correct relaunch.
want "bound_uuids ignores a row with a pane and no tab" \
  "$(bound_uuids "$STATUS_FIXTURE" | grep -c 'u-stray')" "0"

# The eager candidate is the most-recent row whose cwd EXISTS (setup.rs
# `eager_row`). `u-ghost` is the most recent by the clock and its cwd is gone,
# so a reader that sorts and takes the head names the row the launch skips.
want "eager_candidate_uuid skips the most-recent row when its cwd is gone" \
  "$(eager_candidate_uuid "$STATUS_FIXTURE")" "u-eager"
want "eager_candidate_uuid names nothing when no cwd exists" \
  "$(eager_candidate_uuid "$(jq -c '.store.agents |= with_entries(.value.cwd = "/nonexistent/qa")' <<<"$STATUS_FIXTURE")")" ""

# The stale reader names Working and NeedsYou only. `Done` survives a launch
# by design (store.rs `clear_session_order`): it marks an unread result, not
# a running process, so a reader that took every non-idle status would fail
# every fleet with something worth reading.
want "stale_status_uuids names the row wearing a running-process status" \
  "$(stale_status_uuids "$STATUS_FIXTURE" | tr '\n' ' ')" "u-stale "
want "stale_status_uuids leaves a done row alone" \
  "$(stale_status_uuids "$STATUS_FIXTURE" | grep -c 'u-dormant')" "0"

# Fail-closed, like the log readers: a dead `dev status` must read as nothing
# rather than error or hang. The verdict guards the emptiness itself — a set
# comparison where both sides read empty is the masked-read trap that phase 6
# names, so an empty reading is never allowed to stand as a pass.
want "an unreadable status reads as no bound rows" "$(bound_uuids 'not json')" ""
want "an unreadable status reads as no eager candidate" "$(eager_candidate_uuid '')" ""

# The counter is the guard on all of the above. Two EMPTY sets match
# perfectly, so a dead read would report a perfect relaunch. An empty set must
# therefore count 0, loudly, and the verdict refuses on the number.
want "uuid_count counts a set" "$(uuid_count "$(printf 'a\nb\nc')")" "3"
want "uuid_count reads an empty set as zero, not as one blank line" \
  "$(uuid_count "")" "0"
want "uuid_line puts a set on one line, for a verdict a human reads" \
  "$(uuid_line "$(printf 'u-a\nu-b')")" "u-a u-b"

# --- the ledger ------------------------------------------------------------

LEDGER="$(instance_ledger)"
want "the ledger names instance 3's term-facts capability" \
  "$(grep -c 'id 3.*term-facts' <<<"$LEDGER")" "1"
want "the ledger names instance 4's command delta" \
  "$(grep -c 'id 4.*cmd-delta' <<<"$LEDGER")" "1"
want "the ledger names instance 5's dropped line" \
  "$(grep -c 'id 5.*dropped' <<<"$LEDGER")" "1"
want "the ledger leaves the maintainer's instance 2 out entirely" \
  "$(grep -c 'id 2' <<<"$LEDGER")" "0"
# Instance 6 loaded and then did nothing all run. Its row must still be
# there, and must say so: a blank cell reads as "not instrumented".
want "an instance that logged nothing gets a row and an em dash" \
  "$(grep -c 'id 6 *—' <<<"$LEDGER")" "1"

# --- the assertions --------------------------------------------------------
# Verdict TEXT is the contract: a human reads these lines, and a check that
# prints PASS without having compared anything is the failure mode the
# `check_numeric` guard exists for.
phase "selftest"
want "check prints PASS with both sides" \
  "$(check "d" "7" "7" | sed 's/.*CHECK //')" "d: measured=7 expected=7 PASS"
want "check writes 'empty' as the word" \
  "$(check "d" "" "" | sed 's/.*CHECK //')" "d: measured=empty expected=empty PASS"
want "check_min passes at the boundary" \
  "$(check_min "d" "3" 3 | sed 's/.*CHECK //')" "d: measured=3 expected=>=3 PASS"
want "check_numeric rejects jq's null" \
  "$(check_numeric "d" "null" 2>&1 | grep CHECK | sed 's/.*CHECK //')" "d: measured=null expected=<integer> FAIL"
want "note carries the phase and its substitutions" \
  "$(note 'tab %s is a %s' 4 shell | sed 's/.*NOTE //')" "tab 4 is a shell"
want "measure records a reading without a verdict" \
  "$(measure "d" "" | sed 's/.*MEASURE //')" "d: empty"

# A failing check must STOP the run — the drive's phases assume earlier truth,
# so a check that printed FAIL and continued would report a green run built on
# a red reading.
(check "d" "1" "2" >/dev/null 2>&1)
want "a failing check exits non-zero" "$?" "1"
(check_min "d" "1" 5 >/dev/null 2>&1)
want "a failing check_min exits non-zero" "$?" "1"
(check_numeric "d" "" >/dev/null 2>&1)
want "a failing check_numeric exits non-zero" "$?" "1"
(check_nonempty "d" "" >/dev/null 2>&1)
want "a failing check_nonempty exits non-zero" "$?" "1"
want "the failure prints the summary on its way out" \
  "$(check "d" "1" "2" 2>&1 | grep -c 'QA drive summary'; true)" "1"

# A phase that could not RUN is not a phase that failed, and it is certainly
# not one that passed. Phase 6c waits on a human, and the first live run
# (2026-09-16) timed out and threw away ten green phases with it. The run
# continues; the summary says NOT RUN, so nobody can read the gap as a pass.
phase "skipping"
SKIPPED="$(skip_phase 'nobody relaunched' 2>&1)"
want "a skip does not stop the run" "$?" "0"
want "a skip says why, where the reader is looking" \
  "$(grep -c 'NOT RUN: nobody relaunched' <<<"$SKIPPED")" "1"
# Again in THIS shell: the line above ran in a substitution, and a verdict
# recorded only in a subshell never reaches the summary the human reads.
skip_phase 'nobody relaunched' >/dev/null
want "and the summary carries it as its own verdict" \
  "${PHASE_RESULTS[$((${#PHASE_RESULTS[@]} - 1))]}" "NOT RUN"

# The exit code must not say green when a phase is missing. Whoever started
# the drive — a release runbook, an agent, a person who walked away — reads
# the status long before they read the summary.
(exit_on_incomplete >/dev/null 2>&1)
want "a run with a phase that did not run exits non-zero" "$?" "1"
want "and says which phase is missing" \
  "$(exit_on_incomplete 2>&1 | grep -c 'INCOMPLETE: skipping')" "1"
(PHASE_RESULTS=("PASS" "PASS") exit_on_incomplete >/dev/null 2>&1)
want "a run with every phase measured exits zero" "$?" "0"

# --- the relaunch verdict (phase 6c) ---------------------------------------
# The phase costs two maintainer launches to run, so its verdict is the one
# thing in the drive that must NOT wait for a live session to be tried. A
# comparison written the wrong way round would report a correct relaunch on
# every run and nobody would learn otherwise for months — which is how the
# live-set decay (#261) reached a shipped branch in the first place. So the
# verdict runs here, against each shape of a wrong relaunch, and must go red.
relaunch_status() {
  # $1: the uuids bound after the relaunch, as a jq array
  # $2: "stale-status" leaves one dormant row wearing Working.
  # $3: the uuids carrying a standby stamp, as a jq array (default none).
  # Every row carries a `status`, because a real store always writes one: a
  # fixture that omitted it would make the stale check fire on every case and
  # so prove nothing.
  jq -nc --argjson bound "$1" --arg mode "${2:-}" --argjson stamped "${3:-[]}" '
    { store:
      { seq: 12,
        agents: ( [ ("u-eager", "u-a", "u-b") | { key: ., value:
                      { uuid: .,
                        tab_id: (if . as $u | $bound | index($u) then 1 else null end),
                        pane_id: (if . as $u | $bound | index($u) then 5 else null end),
                        standby_stamp: (if . as $u | $stamped | index($u)
                                        then { since: 1, tab: null } else null end),
                        status: (if $mode == "stale-status" and . == "u-a"
                                 then "working" else "idle" end) } } ]
                  | from_entries ) } }'
}

# The two rows the quit left live, as the drive computes them pre-quit.
LEFT_LIVE="$(printf 'u-a\nu-b\n')"
(relaunch_checks "u-eager" "$(relaunch_status '["u-eager"]' "" '["u-a","u-b"]')" "$LEFT_LIVE" >/dev/null 2>&1)
want "one baked tab for the most-recent row, the rest on standby, passes" "$?" "0"

# The pre-quit expectation: the bound rows and the already-stamped ones,
# minus the eager row. u-a is bound, u-b is stamped, and u-eager is both.
PRE_QUIT="$(relaunch_status '["u-eager","u-a"]' "" '["u-b"]')"
want "standby_expected_uuids takes the bound and the stamped, less the eager row" \
  "$(uuid_line "$(standby_expected_uuids "$PRE_QUIT" "u-eager")")" "u-a u-b"
want "standby_uuids reads the stamps" "$(uuid_line "$(standby_uuids "$PRE_QUIT")")" "u-b"

# A row the quit left live came back plain dormant: SessionEnd said something
# other than `other`, or a bar pruned its tab while the session went down.
LOST="$(relaunch_checks "u-eager" "$(relaunch_status '["u-eager"]' "" '["u-a"]')" "$LEFT_LIVE" 2>&1)"
want "a row lost from standby fails" "$?" "1"
want "and the verdict names both sets" \
  "$(grep -c 'on standby: measured=u-a  *expected=u-a u-b  *FAIL' <<<"$LOST")" "1"
(relaunch_checks "u-eager" "$(relaunch_status '["u-eager"]')" "$LEFT_LIVE" >/dev/null 2>&1)
want "a quit that stamped nothing fails" "$?" "1"
(relaunch_checks "u-eager" "$(relaunch_status '["u-eager"]')" "" >/dev/null 2>&1)
want "an empty standby expectation is refused, not passed" "$?" "1"

# Two bound rows: a launch that baked a set. That is the fleet-in-one-layout
# shape that killed the zellij server (#261, measured 2026-09-17), and it is
# the regression the removal of the restore must never let back in.
TWO="$(relaunch_checks "u-eager" "$(relaunch_status '["u-eager","u-a"]')" 2>&1)"
want "two bound rows fail" "$?" "1"
want "and the verdict names the count" \
  "$(grep -c 'exactly ONE row: measured=2 expected=1 FAIL' <<<"$TWO")" "1"

# One tab, the wrong row. The count is right, so only the uuid check sees it.
WRONG="$(relaunch_checks "u-eager" "$(relaunch_status '["u-a"]')" 2>&1)"
want "the wrong row bound fails" "$?" "1"
want "and the verdict names the row it got" \
  "$(grep -c 'most-recent one: measured=u-a expected=u-eager FAIL' <<<"$WRONG")" "1"

# Nothing bound: a launch that baked no tab for a fleet it had.
(relaunch_checks "u-eager" "$(relaunch_status '[]')" >/dev/null 2>&1)
want "no bound row fails" "$?" "1"

# A dormant row wearing Working after the relaunch. The bind checks pass — one
# tab, the right row — but the bar draws that row as an agent mid-turn over a
# process that stopped at the quit (#261, QA run 18).
STALE_STATUS="$(relaunch_checks "u-eager" "$(relaunch_status '["u-eager"]' stale-status)" 2>&1)"
want "a stale running-process status after the relaunch fails" "$?" "1"
want "and the verdict names the row" \
  "$(grep -c 'from the session before: measured=u-a expected=empty FAIL' <<<"$STALE_STATUS")" "1"

# An empty expectation against an empty store. Both sides empty compare
# EQUAL, so a dead pre-quit read must be refused, not passed.
(relaunch_checks "" "$(relaunch_status '[]')" >/dev/null 2>&1)
want "an empty status is refused, not passed" "$?" "1"
(relaunch_checks "" "" >/dev/null 2>&1)
want "a dead read on both sides is refused, not passed" "$?" "1"

# --- the arrival verdict (phase 6c) ----------------------------------------
# One Alt+Down from the baked tab must open the standby row it lands on.
# The row it opened is read as the bound row that is not the baked one, so
# each wrong shape below is a store the drive could really read.
(arrival_checks "u-eager" "$LEFT_LIVE" "$(relaunch_status '["u-eager","u-a"]' "" '["u-b"]')" >/dev/null 2>&1)
want "an arrival that opened a standby row and spent its stamp passes" "$?" "0"
(arrival_checks "u-eager" "$LEFT_LIVE" "$(relaunch_status '["u-eager"]' "" '["u-a","u-b"]')" >/dev/null 2>&1)
want "an arrival that opened nothing fails" "$?" "1"
(arrival_checks "u-eager" "u-b" "$(relaunch_status '["u-eager","u-a"]')" >/dev/null 2>&1)
want "an arrival that opened a row not on standby fails" "$?" "1"
(arrival_checks "u-eager" "$LEFT_LIVE" "$(relaunch_status '["u-eager","u-a"]' "" '["u-a"]')" >/dev/null 2>&1)
want "an arrival whose bind kept the stamp fails" "$?" "1"
(arrival_checks "u-eager" "$LEFT_LIVE" "$(relaunch_status '["u-eager","u-a","u-b"]')" >/dev/null 2>&1)
want "an arrival that opened two rows fails" "$?" "1"

printf '\n%s\n' "== qa/lib selftest: $FAILURES failure(s) =="
[[ "$FAILURES" -eq 0 ]]
