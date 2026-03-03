#!/usr/bin/env bash

set -euo pipefail

threshold="${STACK_RISK_LOCAL_ARRAY_MAX_BYTES:-4096}"
if ! [[ "$threshold" =~ ^[0-9]+$ ]]; then
    echo "check_stack_risk.sh: STACK_RISK_LOCAL_ARRAY_MAX_BYTES must be an integer" >&2
    exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

files=()
if command -v rg >/dev/null 2>&1; then
    while IFS= read -r file; do
        files+=("$file")
    done < <(rg --files src/firmware src/main.rs 2>/dev/null | rg '\.rs$' || true)
else
    while IFS= read -r file; do
        files+=("$file")
    done < <(find src/firmware src -maxdepth 20 -type f -name '*.rs' 2>/dev/null || true)
fi

if [[ "${#files[@]}" -eq 0 ]]; then
    echo "check_stack_risk.sh: no firmware Rust files found; skipping"
    exit 0
fi

violations=0
for file in "${files[@]}"; do
    while IFS= read -r match; do
        line_no="${match%%:*}"
        text="${match#*:}"
        if [[ "$text" == *"stack-risk-reviewed"* ]]; then
            continue
        fi
        size="$(printf '%s' "$text" | sed -E 's/.*\[[[:space:]]*u8[[:space:]]*;[[:space:]]*([0-9_]+)[[:space:]]*\].*/\1/' | tr -d '_')"
        if [[ -z "$size" || ! "$size" =~ ^[0-9]+$ ]]; then
            continue
        fi
        if (( size > threshold )); then
            echo "stack-risk: ${file}:${line_no} has [u8; ${size}] (threshold=${threshold})" >&2
            echo "  add 'stack-risk-reviewed' comment to line after manual review if this is intentional." >&2
            violations=1
        fi
    done < <(rg -n '\[[[:space:]]*u8[[:space:]]*;[[:space:]]*[0-9_]+[[:space:]]*\]' "$file" || true)
done

if (( violations != 0 )); then
    exit 1
fi

echo "stack-risk: no high-risk fixed [u8; N] arrays above threshold ${threshold} detected"
