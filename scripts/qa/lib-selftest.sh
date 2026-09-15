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

# An unreadable log must read as nothing, never as an error or a hang: the
# drive runs before any bar has logged, and on a machine where zellij's log
# has been rotated away.
want "a missing log reads as zero instances" \
  "$(ZLOG="/nonexistent/zellij.log" sandbox_instance_count)" "0"
want "a missing log reads as zero lines" \
  "$(ZLOG="/nonexistent/zellij.log" zlog_now)" "0"

# --- the store readers (phase 6c, the relaunch) ----------------------------
# The relaunch phase compares three sets across a session boundary, and the
# only reason it can be trusted is that each set is read the same way on both
# sides. The fixture holds one row of every shape the comparison must tell
# apart, because a reader that conflates two of them reports a restored fleet
# that is not there.
STATUS_FIXTURE='{
  "session": "clave-test-selftest",
  "session_live": true,
  "store": {
    "seq": 41,
    "last_live": ["u-held-b", "u-eager", "u-held-a"],
    "agents": {
      "u-eager":   {"uuid": "u-eager",   "tab_id": 1,    "pane_id": 5},
      "u-held-a":  {"uuid": "u-held-a",  "tab_id": 2,    "pane_id": null},
      "u-held-b":  {"uuid": "u-held-b",  "tab_id": 3,    "pane_id": null},
      "u-dormant": {"uuid": "u-dormant", "tab_id": null, "pane_id": null},
      "u-stray":   {"uuid": "u-stray",   "tab_id": null, "pane_id": 9}
    }
  }
}'

want "bound_uuids takes every row with a tab, sorted" \
  "$(bound_uuids "$STATUS_FIXTURE" | tr '\n' ' ')" "u-eager u-held-a u-held-b "

# A pane and no tab is not a bind. The bind IS the tab_id (store.rs §6.6), and
# a reader that accepted either field would count a stray pane as a restored
# row and report the set whole when it is short.
want "bound_uuids ignores a row with a pane and no tab" \
  "$(bound_uuids "$STATUS_FIXTURE" | grep -c 'u-stray')" "0"

# The held signature: a tab and NO pane. This is what a restored row looks
# like before its agent runs, and it is the reading that separates "the bar
# rebound the fleet" from "the maintainer landed on a tab and woke it".
want "held_bound_uuids keeps only rows bound before their agent runs" \
  "$(held_bound_uuids "$STATUS_FIXTURE" | tr '\n' ' ')" "u-held-a u-held-b "

# `last_live` is written at LAUNCH, from the binds the previous session left
# (setup.rs `clear_session_order`). It is the only record of what the fleet
# was, so it is the expectation the rebound set is measured against.
want "last_live_uuids reads the recorded set, sorted for comparison" \
  "$(last_live_uuids "$STATUS_FIXTURE" | tr '\n' ' ')" "u-eager u-held-a u-held-b "

# Fail-closed, like the log readers: a dead `dev status` must read as nothing
# rather than error or hang. The phase guards the emptiness itself — a set
# comparison where both sides read empty is the masked-read trap that phase 6
# names, so an empty reading is never allowed to stand as a pass.
want "an unreadable status reads as no bound rows" "$(bound_uuids 'not json')" ""
want "an unreadable status reads as no recorded set" "$(last_live_uuids '')" ""

# The counter is the guard on all of the above. Phase 6c compares two sets,
# and two EMPTY sets match perfectly — so a dead read would report a perfect
# restore. An empty set must therefore count 0, loudly, and the phase refuses
# on the number.
want "uuid_count counts a set" "$(uuid_count "$(bound_uuids "$STATUS_FIXTURE")")" "3"
want "uuid_count reads an empty set as zero, not as one blank line" \
  "$(uuid_count "")" "0"
want "uuid_line puts a set on one line, for a verdict a human reads" \
  "$(uuid_line "$(held_bound_uuids "$STATUS_FIXTURE")")" "u-held-a u-held-b"

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

# --- the relaunch verdict (phase 6c) ---------------------------------------
# The phase costs two maintainer launches to run, so its verdict is the one
# thing in the drive that must NOT wait for a live session to be tried. A
# comparison written the wrong way round would report a restored fleet on
# every run and nobody would learn otherwise for months — which is how the
# decay it exists to catch (#261) reached a shipped branch in the first place.
# So the verdict runs here, against a store that decayed, and must go red.
relaunch_status() {
  # $1: the uuids bound after the relaunch, as a jq array
  # $2: the uuids the launch recorded to restore, as a jq array
  # Bound rows carry a tab and no pane — the held signature — except the
  # eager row, which spawns at launch and takes a pane straight away.
  jq -nc --argjson bound "$1" --argjson recorded "$2" '
    { store:
      { seq: 12, last_live: $recorded,
        agents: ( [ $bound[] | { key: ., value:
                      { uuid: ., tab_id: 1,
                        pane_id: (if . == "u-eager" then 5 else null end) } } ]
                  | from_entries ) } }'
}
RESTORED_BEFORE="$(printf 'u-eager\nu-held-a\nu-held-b')"

(relaunch_checks "$RESTORED_BEFORE" \
  "$(relaunch_status '["u-eager","u-held-a","u-held-b"]' '["u-eager","u-held-a","u-held-b"]')" \
  "u-closed" >/dev/null 2>&1)
want "a fleet that came back whole passes" "$?" "0"

# #261 itself: the store recorded three, and one bound. The old code bound
# only the tab the maintainer was looking at, so this is what a drive would
# have measured the day the defect shipped.
DECAYED="$(relaunch_checks "$RESTORED_BEFORE" \
  "$(relaunch_status '["u-eager"]' '["u-eager","u-held-a","u-held-b"]')" "u-closed" 2>&1)"
want "a decayed fleet fails" "$?" "1"
want "and the verdict names the size it came back at" \
  "$(grep -c 'SAME SIZE: measured=1 expected=3 FAIL' <<<"$DECAYED")" "1"

# The other direction: a row whose tab was closed in the first session comes
# back in the second. Every set here matches — the row was still bound when
# the session died, which is a prune that did not happen (phase 3's family) —
# so this is the one defect the size and uuid checks cannot see.
STALE="$(relaunch_checks "$(printf 'u-closed\nu-eager\nu-held-a')" \
  "$(relaunch_status '["u-closed","u-eager","u-held-a"]' '["u-closed","u-eager","u-held-a"]')" \
  "u-closed" 2>&1)"
want "a closed tab that came back fails" "$?" "1"
want "and the verdict names the closed row" \
  "$(grep -c 'closed in the first session is absent' <<<"$STALE")" "1"

# Both sides empty compare EQUAL. A dead `dev status` must not read as a
# perfect restore.
(relaunch_checks "" "$(relaunch_status '[]' '[]')" "" >/dev/null 2>&1)
want "two empty sets are refused, not passed" "$?" "1"

printf '\n%s\n' "== qa/lib selftest: $FAILURES failure(s) =="
[[ "$FAILURES" -eq 0 ]]
