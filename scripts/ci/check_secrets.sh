#!/usr/bin/env bash

set -euo pipefail

mode="tracked"
if [[ "${1:-}" == "--staged" ]]; then
    mode="staged"
    shift
fi

if [[ "$#" -ne 0 ]]; then
    echo "usage: $0 [--staged]" >&2
    exit 2
fi

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "check_secrets.sh: must run inside a git work tree" >&2
    exit 2
fi

declare -a files=()
if [[ "$mode" == "staged" ]]; then
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git diff --cached --name-only --diff-filter=ACMRTUXB -z)
else
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z)
fi

if [[ "${#files[@]}" -eq 0 ]]; then
    exit 0
fi

declare -a findings=()
legacy_ssid_pattern="SUPREMATIC""_INTERNAL"
legacy_password_pattern="Suprematism""Forever!"

append_matches() {
    local file="$1"
    local pattern="$2"
    local rule="$3"
    local match
    while IFS= read -r match; do
        [[ -z "$match" ]] && continue
        findings+=("$file:$match [$rule]")
    done < <(grep -nIE "$pattern" "$file" || true)
}

for file in "${files[@]}"; do
    [[ -f "$file" ]] || continue
    append_matches \
        "$file" \
        "(HOSTCTL_NET_(SSID|PASSWORD)|MEDITAMER_WIFI_(SSID|PASSWORD))[[:space:]]*=[[:space:]]*'[^<*$][^']+'" \
        "env assignment (single-quoted)"
    append_matches \
        "$file" \
        "(HOSTCTL_NET_(SSID|PASSWORD)|MEDITAMER_WIFI_(SSID|PASSWORD))[[:space:]]*=[[:space:]]*\"[^<*$][^\"]+\"" \
        "env assignment (double-quoted)"
    append_matches \
        "$file" \
        "(HOSTCTL_NET_(SSID|PASSWORD)|MEDITAMER_WIFI_(SSID|PASSWORD))[[:space:]]*=[[:space:]]*[A-Za-z0-9._:/@-][^[:space:]#]*" \
        "env assignment (unquoted)"
    append_matches \
        "$file" \
        "${legacy_ssid_pattern}|${legacy_password_pattern}" \
        "known leaked literal"
done

if [[ "${#findings[@]}" -gt 0 ]]; then
    echo "Secret scan failed: potential credentials detected." >&2
    echo "Use .env.local (gitignored) and placeholders in tracked files." >&2
    echo >&2
    printf '%s\n' "${findings[@]}" >&2
    exit 1
fi
