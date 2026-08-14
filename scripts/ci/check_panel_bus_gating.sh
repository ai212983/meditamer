#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
failed=0

while IFS=: read -r file line _; do
    start=$((line > 15 ? line - 15 : 1))
    end=$((line + 25))
    context="$(sed -n "${start},${end}p" "$repo_root/$file")"
    if [[ "$context" != *'panel_bus::suspend_clients().await'* ]]; then
        echo "panel bus gating: FAIL: missing suspend before $file:$line" >&2
        failed=1
    fi
    if [[ "$context" != *'panel_bus::resume_clients('* ]]; then
        echo "panel bus gating: FAIL: missing resume after $file:$line" >&2
        failed=1
    fi
done < <(cd "$repo_root" && rg -n 'display_bw_(partial_)?async\(' src/firmware -g '*.rs')

if ((failed != 0)); then
    exit 1
fi

if rg -n 'suspend_touch_acquisition\(|resume_touch_acquisition\(' \
    "$repo_root/src/firmware" -g '*.rs' \
    | grep -v '/touch/tasks/acquisition.rs:' \
    | grep -v '/panel_bus.rs:'; then
    echo "panel bus gating: FAIL: direct touch-only display gate remains" >&2
    exit 1
fi

echo "panel bus gating: PASS"
