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

printf '%-28s %10s %10s %10s %10s %10s %10s\n' \
  stage w0 w1 w2 w3 w4 w5

rg "blob_hal" -a "$log" \
  | rg "label=mac_event_window" \
  | sed -E 's/.*after=([^ ]+) label=mac_event_window .*w0=0x([^ ]+) w1=0x([^ ]+) w2=0x([^ ]+) w3=0x([^ ]+) w4=0x([^ ]+) w5=0x([^ ]+).*/\1 \2 \3 \4 \5 \6 \7/' \
  | awk 'BEGIN{OFS=" "} {
      printf("%-28s %10s %10s %10s %10s %10s %10s\n", $1,
        $2, $3, $4, $5, $6, $7);
    }'
