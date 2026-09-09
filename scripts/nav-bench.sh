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
# FAN-OUT IS THE VARIABLE. A one-tab sandbox measured 75 ms/gesture and a
# ten-instance one 160 ms on the same build — the announce costs what it costs
# per recipient, which is why the sandbox always felt faster than a real fleet
# and why a single-tab run proves nothing. Open tabs until the instance count
# matches the fleet you care about.
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
MARK=$(wc -l < "$LOG")

for i in $(seq 1 "$N"); do
  # Alternate, so the burst walks the ring instead of pressing into its end
  # — a nav that cannot move is a no-op and logs no landing.
  if (( i % 2 == 0 )); then DIR=next; else DIR=prev; fi
  "$CT" pipe --name clave-nav -- "{\"dir\":\"$DIR\"}" >/dev/null 2>&1 \
    || echo "  gesture $i refused (rc=$?)"
done

sleep 1
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
