#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <capture.log>" >&2
  exit 1
fi

log="$1"

if [[ ! -f "$log" ]]; then
  echo "log not found: $log" >&2
  exit 1
fi

printf '%-28s %10s %10s %10s %10s %10s %10s %10s %10s %10s %10s %10s %10s\n' \
  stage w0 w1 w2 w3 w4 w5 w6 w7 w8 w9 w10 w11

rg "mac_event_window" -a "$log" \
  | rg "words0_5=" \
  | sed -E 's/.*after=([^ ]+) words0_5=([^ ]+) words6_11=([^ ]+).*/\1 \2 \3/' \
  | awk 'BEGIN{OFS=" "} {
      split($2, a, ":");
      split($3, b, ":");
      printf("%-28s %10s %10s %10s %10s %10s %10s %10s %10s %10s %10s %10s %10s\n", $1,
        a[1], a[2], a[3], a[4], a[5], a[6],
        b[1], b[2], b[3], b[4], b[5], b[6]);
    }'
