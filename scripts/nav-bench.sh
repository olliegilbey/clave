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

MARK=$(wc -l < "$LOG")

for i in $(seq 1 "$N"); do
  # Alternate, so the burst walks the ring instead of pressing into its end
  # — a nav that cannot move is a no-op and logs no landing.
  if (( i % 2 == 0 )); then DIR=next; else DIR=prev; fi
  "$CT" pipe --name clave-nav -- "{\"dir\":\"$DIR\"}" >/dev/null 2>&1 \
    || echo "  gesture $i refused (rc=$?)"
done

# Wait for the log to SETTLE, rather than sleeping a fixed second. The last
# gestures' lines land after their pipes return, and cutting the slice early
# loses them — which shows up as a landing shortfall, i.e. as an election
# refusal, the one reading this script exists to make trustworthy. Stop when
# the slice has held the same length twice in a row; cap it so a session that
# is genuinely busy cannot hang the bench.
#
# Settle on OUR OWN lines, not the file length: this log is user-global, so a
# live fleet in another session writes to it continuously and a file-length
# check would never settle.
SETTLE_POLLS=40
prev=-1
for _ in $(seq 1 "$SETTLE_POLLS"); do
  sleep 0.25
  now=$(tail -n "+$((MARK + 1))" "$LOG" | grep -c -E "nav landed|clave-bar: beacon ")
  [ "$now" = "$prev" ] && break
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
pct = lambda p: gaps[min(len(gaps) - 1, int(p * len(gaps)))] if gaps else float("nan")
instances = len({i for _, i in beacons})

print(f"--- nav-bench {label} ---")
print(f"gestures sent        : {n}")
print(f"landings logged      : {len(landed)}   (a shortfall is an election refusal, not a lost pipe)")
print(f"instances reached    : {instances}     (distinct bars that logged a beacon)")
print(f"beacon deliveries    : {len(beacons)}")
if landed:
    print(f"first -> last landing: {(landed[-1][0] - landed[0][0]).total_seconds():.2f}s")
if gaps:
    print(f"gap median / p90     : {pct(0.5):.3f}s / {pct(0.9):.3f}s")
    print(f"gap min / max        : {gaps[0]:.3f}s / {gaps[-1]:.3f}s")
print(f"raw slice            : {path}")
PY
