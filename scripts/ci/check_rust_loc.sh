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

warn_limit="${RUST_LOC_WARN:-220}"
max_limit="${RUST_LOC_MAX:-300}"

if ! [[ "$warn_limit" =~ ^[0-9]+$ ]] || ! [[ "$max_limit" =~ ^[0-9]+$ ]]; then
  echo "check_rust_loc.sh: RUST_LOC_WARN and RUST_LOC_MAX must be integers" >&2
  exit 2
fi

if (( warn_limit > max_limit )); then
  echo "check_rust_loc.sh: RUST_LOC_WARN must be <= RUST_LOC_MAX" >&2
  exit 2
fi

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "check_rust_loc.sh: must run inside a git work tree" >&2
  exit 2
fi

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

declare -a files=()
if [[ "$mode" == "staged" ]]; then
  while IFS= read -r -d '' file; do
    [[ "$file" == *.rs ]] || continue
    [[ -f "$file" ]] || continue
    files+=("$file")
  done < <(git diff --cached --name-only --diff-filter=ACMRTUXB -z -- src tools/hostctl/src)
else
  while IFS= read -r -d '' file; do
    [[ "$file" == *.rs ]] || continue
    [[ -f "$file" ]] || continue
    files+=("$file")
  done < <(git ls-files -z -- src tools/hostctl/src)
fi

if (( ${#files[@]} == 0 )); then
  echo "rust-loc: no Rust files to check"
  exit 0
fi

tmp_warn="$(mktemp)"
tmp_hard="$(mktemp)"
trap 'rm -f "$tmp_warn" "$tmp_hard"' EXIT

checked_count=0
for file in "${files[@]}"; do
  lines="$(wc -l <"$file" | tr -d ' ')"
  checked_count=$((checked_count + 1))

  if (( lines > max_limit )); then
    printf '%s\t%s\n' "$lines" "$file" >>"$tmp_hard"
  elif (( lines > warn_limit )); then
    printf '%s\t%s\n' "$lines" "$file" >>"$tmp_warn"
  fi
done

echo "rust-loc: checked ${checked_count} file(s) (warn>${warn_limit}, fail>${max_limit})"

if [[ -s "$tmp_warn" ]]; then
  echo
  echo "rust-loc warnings (over ${warn_limit} lines):"
  sort -nr "$tmp_warn" | awk -F '\t' '{ printf "  - %s (%s lines)\n", $2, $1 }'
fi

if [[ -s "$tmp_hard" ]]; then
  echo
  echo "rust-loc violations (over ${max_limit} lines):" >&2
  sort -nr "$tmp_hard" | awk -F '\t' '{ printf "  - %s (%s lines)\n", $2, $1 }' >&2
  exit 1
fi

exit 0
