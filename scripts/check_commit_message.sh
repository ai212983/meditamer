#!/usr/bin/env bash

set -euo pipefail

if ! command -v commitlint >/dev/null 2>&1; then
  cat >&2 <<'EOF'
commitlint is required for conventional commit message validation.
Install it with:
  go install github.com/conventionalcommit/commitlint@latest
EOF
  exit 1
fi

message_file="${1:-}"
if [[ -z "$message_file" || ! -f "$message_file" ]]; then
  echo "commit-msg hook expected a commit message file path." >&2
  exit 1
fi

commitlint lint --message="$message_file"
