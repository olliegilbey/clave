#!/usr/bin/env bash
# seek-trace.sh — read a width-seek trace out of the shared zellij log.
#
# REQUIRES INSTRUMENTATION THAT IS NOT IN THE SHIPPED BAR. The trace it reads
# was temporary (TESTING.md § the instrumentation recipe: temporary means
# temporary), but re-deriving it cost a round once, so the reader is kept. To
# turn the trace back on, add a `seek_trace()` accessor to the model returning
# the seek's inputs, then in `render()` in main.rs:
#
#     let dbg = self.model.seek_trace();
#     let fx = self.model.width_seek(cols);
#     eprintln!("CLAVE_DBG_seek cols={cols} fx={fx:?} {dbg}");
#
# The one field that made the 2026-08-07 diagnosis is the name of whatever last
# re-armed the seek: it is what showed the storm was the collapse mode flipping
# and not the seek failing to converge.
#
# The marker is emitted only by the instrumented build, so it already isolates
# the sandbox from the maintainer's stable fleet — but the log is shared and
# never truncated, so every mode here is time-bounded.
#
# Usage:
#   seek-trace.sh rate [since_HH:MM:SS]   per-second render/action counts
#   seek-trace.sh tail [n]                last n trace lines
#   seek-trace.sh acts [since_HH:MM:SS]   only the renders that MOVED the pane
#   seek-trace.sh arms                    per-instance re-arm counts + trigger
set -uo pipefail

LOG="${TMPDIR:-/tmp}/zellij-$(id -u)/zellij-log/zellij.log"
[[ -r "$LOG" ]] || { echo "no zellij log at $LOG" >&2; exit 1; }

today="$(date +%Y-%m-%d)"
# `tail` takes a line count in $2; every other mode takes a start time.
if [[ "${1:-tail}" == "tail" ]]; then since="00:00:00"; else since="${2:-00:00:00}"; fi

# Lines look like:  DEBUG |...| 2026-08-06 18:20:01.123 [id: 7 ] CLAVE_DBG_seek …
lines() {
  grep -F 'CLAVE_DBG_seek' "$LOG" \
    | awk -v d="$today" -v s="$since" '
        { for (i=1;i<=NF;i++) if ($i==d) { day=$i; tm=$(i+1); break } }
        day==d && substr(tm,1,8) >= s
      '
}

case "${1:-tail}" in
  tail)
    lines | tail -n "${2:-40}"
    ;;
  rate)
    # renders and pane-moving actions per wall-clock second, plus which
    # instances were involved — a storm is a second with many acts.
    lines | awk '
      { for (i=1;i<=NF;i++) if ($i ~ /^[0-9][0-9]:[0-9][0-9]:[0-9][0-9]\./) { sec=substr($i,1,8); break }
        for (i=1;i<=NF;i++) if ($i=="[id:") { id=$(i+1); break }
        r[sec]++; ids[sec]=ids[sec] " " id
        if ($0 !~ /fx=\[\]/) a[sec]++
      }
      END { printf "%-10s %8s %8s\n", "second", "renders", "acts"
            n=asorti(r, k)
            for (j=1;j<=n;j++) printf "%-10s %8d %8d\n", k[j], r[k[j]], a[k[j]]+0 }
    ' 2>/dev/null || lines | awk '
      { for (i=1;i<=NF;i++) if ($i ~ /^[0-9][0-9]:[0-9][0-9]:[0-9][0-9]\./) { sec=substr($i,1,8); break }
        r[sec]++; if ($0 !~ /fx=\[\]/) a[sec]++ }
      END { for (s in r) printf "%s renders=%d acts=%d\n", s, r[s], a[s]+0 }' | sort
    ;;
  acts)
    lines | grep -v 'fx=\[\]'
    ;;
  arms)
    # highest re-arm count seen per plugin instance, with the last trigger.
    lines | sed -E 's/.*\[id: *([0-9]+).*arms=([0-9]+) why=([a-z-]+).*/\1 \2 \3/' \
      | awk '{ if ($2+0 >= m[$1]) { m[$1]=$2+0; w[$1]=$3 } }
              END { printf "%-6s %6s %s\n","inst","arms","last-trigger"
                    for (i in m) printf "%-6s %6d %s\n", i, m[i], w[i] }' | sort -k2 -rn
    ;;
  *) echo "usage: $0 {tail|rate|acts|arms} [arg]" >&2; exit 2 ;;
esac
