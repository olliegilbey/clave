#!/usr/bin/env bash
# nav-bench.sh — how fast does nav keep up with a key-mash? (#141)
#
# Why this exists: nav latency was the subject of an issue for five weeks and
# had never been measured, because nothing on the nav path was timestamped. The
# diagnosis was derived from the code and one incident note, and the fix could
# only be argued, not shown. Now the bar logs one `nav landed` line per gesture
# from the instance that acts, and one `beacon` line per instance per announce;
# zellij stamps both to the millisecond, so a burst is measurable.
#
# It bursts N nav gestures at THIS worktree's sandbox and reports the spacing
# between landings. Read the MEDIAN GAP: that is the per-gesture service time.
#
#   scripts/nav-bench.sh [gestures] [label]
#
# Two things to hold while reading the number.
#
# FAN-OUT IS THE VARIABLE. On the PRE-#141 build (the CLI-pipe announce), a
# one-tab sandbox measured 75 ms/gesture and a ten-instance one 160 ms — the
# announce costs what it costs per recipient, which is why the sandbox always
# felt faster than a real fleet and why a single-tab run proves nothing. Open
# tabs until the instance count matches the fleet you care about. For scale on
# the CURRENT build, ten instances measure ~135 ms.
#
# THE STIMULUS IS NOT FREE. There is no way to press a keybind from a script,
# so this pokes the bar with `zellij pipe` — itself a CLI pipe with the same
# ~1s server-side wait the announce used to pay. A real keypress pays none of
# that. So the absolute number here is an OVER-estimate of what a human feels,
# and the honest use of this script is the DELTA between two builds, not the
# figure on its own.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CT="$SCRIPT_DIR/ct.sh"
N="${1:-30}"
LABEL="${2:-run}"
# The zellij log is USER-GLOBAL and holds every session on the machine
# (FOOTGUNS). Mark the line count first and read only what this burst appends;
# a date filter is not enough.
LOG="${TMPDIR:-/tmp}/zellij-$(id -u)/zellij-log/zellij.log"
OUT="${TMPDIR:-/tmp}/nav-bench-$LABEL.log"

[ -r "$LOG" ] || { echo "no zellij log at $LOG" >&2; exit 1; }
# A non-numeric or zero N would make `seq` emit nothing and the run would
# report a clean zero-gesture bench — a silent no-op that reads like a result.
case "$N" in
  ''|*[!0-9]*) echo "gestures must be a positive integer, got: $N" >&2; exit 2 ;;
esac
[ "$N" -gt 0 ] || { echo "gestures must be > 0, got: $N" >&2; exit 2; }

# One clave session, or the instance count below is fiction. Plugin ids are
# per-server and this log carries no session identity, so with two sessions
# live their beacon lines interleave under colliding ids and the maintainer's
# own key presses land in the gap median. Warn rather than refuse: a run whose
# only purpose is the landing spacing is still readable, and the operator may
# know the other session is idle.
SESSIONS=$(zellij list-sessions --short 2>/dev/null | grep -c . || echo 0)
if [ "$SESSIONS" -gt 1 ]; then
  cat >&2 <<WARN
  WARNING: $SESSIONS zellij sessions are live. The zellij log is user-global and
  these lines carry no session identity, so 'instances reached' and the gap
  figures below MIX sessions. Treat this run as indicative only.
WARN
fi

MARK=$(wc -l < "$LOG")

for i in $(seq 1 "$N"); do
  # Alternate, so the burst walks the ring instead of pressing into its end.
  # (The landing line fires on ARRIVAL at the elected instance, so a press
  # that cannot move still logs — the alternation is about not parking the
  # selection against the ring's end, not about the log.)
  if (( i % 2 == 0 )); then DIR=next; else DIR=prev; fi
  # `</dev/null` is NOT optional, and matches frecency-walk.sh. `ct.sh` wraps
  # every call in `timeout 15`; a `zellij pipe` client killed mid-flight with
  # an inherited terminal stdin leaves the server holding a half-open stream,
  # and every later pipe queues behind it — a CUMULATIVE, unrecoverable wedge
  # of the session's whole CLI-pipe lane (FOOTGUNS). Thirty invocations a run.
  #
  # stdout is suppressed; STDERR IS NOT. ct.sh's refusal text is the only
  # thing it ever prints, and swallowing it is called out by name in FOOTGUNS:
  # with it hidden, a dead sandbox yields N bare "refused" lines and then a
  # confident `landings logged: 0`, which this script's own output glosses as
  # an election refusal — asserting the one diagnosis it exists to make
  # trustworthy, and asserting it wrongly.
  "$CT" pipe --name clave-nav -- "{\"dir\":\"$DIR\"}" >/dev/null </dev/null \
    || echo "  gesture $i refused (rc=$?) — see the refusal above" >&2
done

# Wait for the log to SETTLE, rather than sleeping a fixed second. The last
# gestures' lines land after their pipes return, and cutting the slice early
# loses them — which shows up as a landing shortfall, i.e. as an election
# refusal, the one reading this script exists to make trustworthy.
#
# Settle on OUR OWN lines, not the file length: this log is user-global, so a
# live fleet in another session writes to it continuously and a file-length
# check would never converge.
#
# The primary exit is "all N landings are in", which is exact. The quiet-run
# fallback exists only for the genuinely short case, and it wants FOUR steady
# polls, not two: the measured p90 gap is ~0.14s and the max is unbounded, so
# two 0.25s polls can straddle one slow gesture and stop early — recreating
# the very truncation this loop was added to prevent.
SETTLE_POLLS=40
STEADY_NEEDED=4
prev=-1
steady=0
for _ in $(seq 1 "$SETTLE_POLLS"); do
  sleep 0.25
  now=$(tail -n "+$((MARK + 1))" "$LOG" | grep -c "nav landed" || true)
  [ "$now" -ge "$N" ] && break
  if [ "$now" = "$prev" ]; then
    steady=$((steady + 1))
    [ "$steady" -ge "$STEADY_NEEDED" ] && break
  else
    steady=0
  fi
  prev=$now
done
tail -n "+$((MARK + 1))" "$LOG" > "$OUT"

python3 - "$LABEL" "$N" "$OUT" <<'PY'
import re, sys, datetime
label, n, path = sys.argv[1:]
lines = open(path).read().splitlines()
stamp = re.compile(r"(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d+)")
inst = re.compile(r"\[id: (\d+)")

def at(needle):
    out = []
    for l in lines:
        if needle in l:
            m, i = stamp.search(l), inst.search(l)
            if m:
                out.append((datetime.datetime.strptime(m.group(1), "%Y-%m-%d %H:%M:%S.%f"),
                            i.group(1) if i else "?"))
    return out

landed, beacons = at("nav landed"), at("clave-bar: beacon ")
gaps = sorted((b[0] - a[0]).total_seconds() for a, b in zip(landed, landed[1:]))
# Nearest-rank, so "p90" on a short run is the next rank up — at n=29 it is
# really the 93rd percentile. Fine for a delta between builds; do not quote it
# as a percentile in isolation.
pct = lambda p: gaps[min(len(gaps) - 1, int(p * len(gaps)))] if gaps else float("nan")
instances = len({i for _, i in beacons})

print(f"--- nav-bench {label} ---")
print(f"gestures sent        : {n}")
print(f"landings logged      : {len(landed)}   (a shortfall is an election refusal, not a lost pipe)")
print(f"instances reached    : {instances}     (distinct plugin ids, NOT session-filtered — see the warning above if any)")
print(f"beacon deliveries    : {len(beacons)}")
if landed:
    print(f"first -> last landing: {(landed[-1][0] - landed[0][0]).total_seconds():.2f}s")
if gaps:
    print(f"gap median / p90     : {pct(0.5):.3f}s / {pct(0.9):.3f}s")
    print(f"gap min / max        : {gaps[0]:.3f}s / {gaps[-1]:.3f}s")
print(f"raw slice            : {path}")
PY
