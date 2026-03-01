#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
work_dir="$repo_root/.scannerwork"
lock_dir="$repo_root/.scannerwork.lock"
env_file="${SONAR_ENV_FILE:-$repo_root/.env.local}"

host_url="${SONAR_HOST_URL:-http://localhost:9000}"
project_key="${SONAR_PROJECT_KEY:-Meditamer}"
shell_token="${SONAR_TOKEN:-}"

poll_ce="${SONAR_POLL_CE:-1}"
ce_timeout_sec="${SONAR_CE_TIMEOUT_SEC:-300}"
ce_poll_interval_sec="${SONAR_CE_POLL_INTERVAL_SEC:-2}"

if ! command -v sonar-scanner >/dev/null 2>&1; then
  cat >&2 <<'EOF'
sonar-scanner is required to run SonarQube analysis.
Install with:
  npm install -g @sonar/scan
EOF
  exit 1
fi

# Auto-load local env vars (for example SONAR_TOKEN) from .env.local.
if [[ -f "$env_file" ]]; then
  # shellcheck source=/dev/null
  set -a
  source "$env_file"
  set +a
fi

# Keep explicit shell export as highest priority over env file values.
if [[ -n "$shell_token" ]]; then
  token="$shell_token"
else
  token="${SONAR_TOKEN:-}"
fi

if [[ -z "$token" ]]; then
  cat >&2 <<'EOF'
SONAR_TOKEN is required.
Example:
  SONAR_TOKEN=*** scripts/ci/sonar_scan.sh

Recommended:
  1) copy .env.example to .env.local
  2) set SONAR_TOKEN in .env.local
EOF
  exit 1
fi

if ! mkdir "$lock_dir" 2>/dev/null; then
  echo "another SonarQube scan appears to be running for this repo" >&2
  echo "remove .scannerwork.lock if this is stale" >&2
  exit 1
fi
trap 'rmdir "$lock_dir" >/dev/null 2>&1 || true' EXIT

# Avoid stale/interleaved scanner state between local runs.
rm -rf "$work_dir"

(
  cd "$repo_root"
  sonar-scanner \
    -Dsonar.host.url="$host_url" \
    -Dsonar.projectKey="$project_key" \
    -Dsonar.token="$token" \
    "$@"
)

report_file="$repo_root/.scannerwork/report-task.txt"
if [[ ! -f "$report_file" ]]; then
  echo "warning: report file not found: .scannerwork/report-task.txt" >&2
  exit 0
fi

ce_task_url="$(
  sed -n 's/^ceTaskUrl=//p' "$report_file" | head -n 1
)"
dashboard_url="$(
  sed -n 's/^dashboardUrl=//p' "$report_file" | head -n 1
)"

if [[ "$poll_ce" == "1" && -n "$ce_task_url" && -n "$token" ]] && command -v curl >/dev/null 2>&1; then
  echo "Waiting for SonarQube Compute Engine task..."
  started_at="$(date +%s)"
  while true; do
    ce_json="$(curl -fsS -H "Authorization: Bearer $token" "$ce_task_url")"
    ce_status="$(printf '%s' "$ce_json" | sed -n 's/.*"status":"\([^"]*\)".*/\1/p' | head -n 1)"
    case "$ce_status" in
      SUCCESS)
        echo "Compute Engine task finished: SUCCESS"
        break
        ;;
      PENDING|IN_PROGRESS)
        now="$(date +%s)"
        if (( now - started_at >= ce_timeout_sec )); then
          echo "Compute Engine wait timed out after ${ce_timeout_sec}s" >&2
          exit 2
        fi
        sleep "$ce_poll_interval_sec"
        ;;
      FAILED|CANCELED)
        echo "Compute Engine task finished: $ce_status" >&2
        echo "$ce_json" >&2
        exit 2
        ;;
      *)
        echo "Unknown Compute Engine status: ${ce_status:-<empty>}" >&2
        echo "$ce_json" >&2
        exit 2
        ;;
    esac
  done
fi

if command -v curl >/dev/null 2>&1; then
  gate_json="$(
    curl -fsS \
      -H "Authorization: Bearer $token" \
      "$host_url/api/qualitygates/project_status?projectKey=$project_key"
  )"
  gate_status="$(printf '%s' "$gate_json" | sed -n 's/.*"status":"\([^"]*\)".*/\1/p' | head -n 1)"
  echo "Quality Gate: ${gate_status:-unknown}"
  if [[ "$gate_status" != "OK" ]]; then
    echo "Quality Gate is not OK. Check SonarQube for details." >&2
    [[ -n "$dashboard_url" ]] && echo "Dashboard: $dashboard_url" >&2
    exit 3
  fi
fi

if [[ -n "$dashboard_url" ]]; then
  echo "Dashboard: $dashboard_url"
fi
