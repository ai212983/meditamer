#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

cd "$repo_root"

if ! command -v lychee >/dev/null 2>&1; then
  cat >&2 <<'EOF'
lychee is required for markdown link validation.
Install it with one of:
  brew install lychee
  cargo install --locked lychee
EOF
  exit 1
fi

mode="staged"
if [[ "${1:-}" == "--all" ]]; then
  mode="all"
  shift
fi

# Frozen material follows the layout it was archived in and is never edited, so
# its stale cross-references are expected and must not gate commits. Vendored
# documentation follows its upstream layout for the same reason.
exclude_regex="${MARKDOWN_LINKS_EXCLUDE_REGEX:-^vendor/|^docs/archive/}"

declare -a candidates=()

if (( $# > 0 )); then
  for file in "$@"; do
    [[ "$file" == *.md ]] || continue
    [[ -f "$file" ]] || continue
    candidates+=("$file")
  done
elif [[ "$mode" == "all" ]]; then
  while IFS= read -r -d '' file; do
    [[ -f "$file" ]] || continue
    candidates+=("$file")
  done < <(git ls-files -z -- '*.md')
else
  while IFS= read -r -d '' file; do
    candidates+=("$file")
  done < <(git diff --cached --name-only --diff-filter=ACMR -z -- '*.md')
fi

declare -a files=()
declare -i skipped=0
for file in ${candidates+"${candidates[@]}"}; do
  if [[ -n "$exclude_regex" && "$file" =~ $exclude_regex ]]; then
    skipped+=1
    continue
  fi
  files+=("$file")
done

if (( skipped > 0 )); then
  echo "Skipping ${skipped} excluded file(s) (${exclude_regex})."
fi

if (( ${#files[@]} == 0 )); then
  echo "No markdown files selected for link validation."
  exit 0
fi

declare -a lychee_args
lychee_args=(--no-progress --exclude '^data:image/')

if [[ "${MARKDOWN_LINKS_ONLINE:-0}" != "1" ]]; then
  # Keep the hook fast/reliable by skipping remote network checks by default.
  lychee_args+=(--offline)
fi

echo "Checking markdown links in ${#files[@]} file(s)..."
lychee "${lychee_args[@]}" "${files[@]}"
