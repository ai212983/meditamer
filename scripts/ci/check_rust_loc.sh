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

# Aligned with the enforced SLOC ratchet in `config/rca-baseline.json`.
# This remains a raw-line advisory; the rust-code-analysis SLOC gate is the
# blocking authority.
warn_limit="${RUST_LOC_WARN:-600}"
max_limit="${RUST_LOC_MAX:-1000}"

# Test modules are exempt. Table-driven tests are legitimately long and
# repetitive, and splitting a table destroys the thing that makes it readable.
# Mirrors the append-only exemption in the Markdown LOC policy.
exclude_regex="${RUST_LOC_EXCLUDE_REGEX:-(^|/)tests?(/|\\.rs$)}"

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
excluded_count=0
collect() {
  local file="$1"
  [[ "$file" == *.rs ]] || return 0
  [[ -f "$file" ]] || return 0
  if [[ -n "$exclude_regex" ]] && [[ "$file" =~ $exclude_regex ]]; then
    excluded_count=$((excluded_count + 1))
    return 0
  fi
  files+=("$file")
}

if [[ "$mode" == "staged" ]]; then
  while IFS= read -r -d '' file; do
    collect "$file"
  done < <(git diff --cached --name-only --diff-filter=ACMRTUXB -z -- src packages tools)
else
  while IFS= read -r -d '' file; do
    collect "$file"
  done < <(git ls-files -z -- src packages tools)
fi

if (( excluded_count > 0 )); then
  echo "rust-loc: skipping ${excluded_count} test file(s) (${exclude_regex})"
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
  elif (( lines >= warn_limit )); then
    printf '%s\t%s\n' "$lines" "$file" >>"$tmp_warn"
  fi
done

echo "rust-loc: checked ${checked_count} file(s) (warn>=${warn_limit}, high-attention>${max_limit}, advisory-only)"

if [[ -s "$tmp_warn" ]]; then
  echo
  echo "rust-loc warnings (at least ${warn_limit} lines):"
  sort -nr "$tmp_warn" | awk -F '\t' '{ printf "  - %s (%s lines)\n", $2, $1 }'
fi

if [[ -s "$tmp_hard" ]]; then
  echo
  echo "rust-loc high-attention advisories (over ${max_limit} lines):"
  sort -nr "$tmp_hard" | awk -F '\t' '{ printf "  - %s (%s lines)\n", $2, $1 }'
fi

exit 0
