#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
engine_dir="$repo_root/packages/sdcard/src/fat/engine"

if [[ ! -d "$engine_dir" ]]; then
    echo "fat-engine-stackless: missing $engine_dir" >&2
    exit 1
fi

if rg -n 'async[[:space:]]+fn|\.await\b' "$engine_dir"; then
    echo "fat-engine-stackless: async code is forbidden in the FAT engine" >&2
    exit 1
fi

if rg -n '(^|[^[:alnum:]_])(Box|Vec|String)[[:space:]]*<|alloc::|std::collections' "$engine_dir"; then
    echo "fat-engine-stackless: heap-backed state is forbidden in the FAT engine" >&2
    exit 1
fi

if rg -n 'self\.[[:alnum:]_]+[[:space:]]*=[[:space:]]*[[:alnum:]_]+::(new|default)\(' "$engine_dir"; then
    echo "fat-engine-stackless: reset state in place instead of replacing whole submachines" >&2
    exit 1
fi

if rg -n 'advance_(mount|resolve|find_target|fat_read|list|read|mutation|fat_write|allocate|free|data_write)\([^;]*\)' "$engine_dir" | rg 'self\.advance_'; then
    echo "fat-engine-stackless: recursive stage transition detected" >&2
    exit 1
fi

legacy_pattern='run_sd_fat_|FatAppendSession|fat::(read_file|write_file|list_dir|mkdir|remove|rename|rename_replace|append_|truncate|stat)'
if rg -n "$legacy_pattern" \
    "$repo_root/src/firmware" \
    "$repo_root/packages/sdcard/src/lib.rs" \
    "$repo_root/packages/sdcard/src/fat/mod.rs" \
    "$repo_root/packages/sdcard/src/runtime/mod.rs"; then
    echo "fat-engine-stackless: production legacy FAT entry point detected" >&2
    exit 1
fi

echo "fat-engine-stackless: synchronous stepped engine verified"
