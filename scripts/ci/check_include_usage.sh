#!/usr/bin/env bash

# `include!` is for build-script output, not for managing file size.
#
# Textually pasting a hand-written source file into a parent looks like a split
# but is not one: no module boundary, no privacy, no independent `use` list, and
# rust-analyzer degrades across the seam. A split that does not create a module
# boundary is not a split -- see AGENTS.md.
#
# The only legitimate form is generated code from OUT_DIR:
#   include!(concat!(env!("OUT_DIR"), "/event_config.rs"));
#
# Advisory by default. Set INCLUDE_USAGE_ENFORCE=1 to fail on violations once
# the existing hand-split sites have been converted to real modules.

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

enforce="${INCLUDE_USAGE_ENFORCE:-0}"

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "check_include_usage.sh: must run inside a git work tree" >&2
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
  done < <(git diff --cached --name-only --diff-filter=ACMRTUXB -z -- src packages tools)
else
  while IFS= read -r -d '' file; do
    [[ "$file" == *.rs ]] || continue
    [[ -f "$file" ]] || continue
    files+=("$file")
  done < <(git ls-files -z -- src packages tools)
fi

if (( ${#files[@]} == 0 )); then
  echo "include-usage: no Rust files to check"
  exit 0
fi

tmp_bad="$(mktemp)"
trap 'rm -f "$tmp_bad"' EXIT

checked_count=0
generated_count=0
for file in "${files[@]}"; do
  checked_count=$((checked_count + 1))
  while IFS= read -r hit; do
    [[ -n "$hit" ]] || continue
    if [[ "$hit" == *'env!("OUT_DIR")'* ]]; then
      generated_count=$((generated_count + 1))
      continue
    fi
    printf '%s\t%s\n' "$file" "$hit" >>"$tmp_bad"
  done < <(grep -n 'include!' "$file" 2>/dev/null || true)
done

violations=0
if [[ -s "$tmp_bad" ]]; then
  violations="$(wc -l <"$tmp_bad" | tr -d ' ')"
fi

label="advisory-only"
if [[ "$enforce" == "1" ]]; then
  label="enforcing"
fi
echo "include-usage: checked ${checked_count} file(s), ${generated_count} generated include(s) OK (${label})"

if (( violations > 0 )); then
  echo
  echo "include! of hand-written source (${violations}); make these real modules:"
  sort "$tmp_bad" | awk -F '\t' '{ printf "  - %s:%s\n", $1, $2 }'
  echo
  echo "Only include!(concat!(env!(\"OUT_DIR\"), ...)) is permitted."

  if [[ "$enforce" == "1" ]]; then
    exit 1
  fi
fi

exit 0
