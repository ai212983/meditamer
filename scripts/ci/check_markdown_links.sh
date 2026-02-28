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

declare -a files=()

if (( $# > 0 )); then
  for file in "$@"; do
    [[ "$file" == *.md ]] || continue
    [[ -f "$file" ]] || continue
    files+=("$file")
  done
else
  while IFS= read -r -d '' file; do
    files+=("$file")
  done < <(git diff --cached --name-only --diff-filter=ACMR -z -- '*.md')
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
