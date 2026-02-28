#!/usr/bin/env bash

set -euo pipefail

readonly ALLOWED_SCOPES=(
  runtime
  touch
  event-engine
  storage
  upload
  wifi
  telemetry
  graphics
  drivers
  tooling
  ci
  docs
)

if ! command -v commitlint >/dev/null 2>&1; then
  cat >&2 <<'EOM'
commitlint is required for conventional commit message validation.
Install it with:
  go install github.com/conventionalcommit/commitlint@latest
EOM
  exit 1
fi

message_file="${1:-}"
if [[ -z "$message_file" || ! -f "$message_file" ]]; then
  echo "commit-msg hook expected a commit message file path." >&2
  exit 1
fi

# Extract the first non-empty, non-comment line from the commit message.
subject="$(
  sed -e '/^[[:space:]]*#/d' -e '/^[[:space:]]*$/d' "$message_file" | head -n 1
)"

if [[ -z "$subject" ]]; then
  echo "commit message subject is empty." >&2
  exit 1
fi

# Keep git-generated and autosquash subjects untouched.
if [[ "$subject" =~ ^(Merge|Revert)\  ]] || [[ "$subject" =~ ^(fixup\!|squash\!) ]]; then
  exit 0
fi

commitlint lint --message="$message_file"

if [[ ! "$subject" =~ ^([a-z]+)(\(([a-z0-9-]+)\))?(!)?:[[:space:]].+ ]]; then
  cat >&2 <<'EOM'
Could not parse commit header for scope validation.
Expected format: type(scope): subject
EOM
  exit 1
fi

scope="${BASH_REMATCH[3]:-}"
if [[ -z "$scope" ]]; then
  cat >&2 <<EOM
Commit scope is required.
Use one of: ${ALLOWED_SCOPES[*]}
EOM
  exit 1
fi

scope_allowed=0
for allowed_scope in "${ALLOWED_SCOPES[@]}"; do
  if [[ "$scope" == "$allowed_scope" ]]; then
    scope_allowed=1
    break
  fi
done

if [[ "$scope_allowed" -ne 1 ]]; then
  cat >&2 <<EOM
Invalid commit scope: '$scope'
Allowed scopes: ${ALLOWED_SCOPES[*]}
EOM
  exit 1
fi
