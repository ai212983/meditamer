#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
toolchain="${RUSTUP_TOOLCHAIN:-stable}"
output_dir="${HOST_COVERAGE_OUTPUT_DIR:-$repo_root/logs/coverage}"
min_line_coverage="${HOST_COVERAGE_MIN_LINE:-0}"
merged_lcov_path="${HOST_COVERAGE_MERGED_LCOV:-$output_dir/host_coverage.lcov}"

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  cat >&2 <<'EOF'
cargo-llvm-cov is required for host coverage.
Install with:
  cargo install --locked cargo-llvm-cov
EOF
  exit 1
fi

if [[ "${1:-}" != "" && "${1:-}" != --* ]]; then
  host_target="$1"
  shift
else
  host_target="$(rustup run "$toolchain" rustc -vV | awk '/^host:/ {print $2}')"
fi
if [[ -z "$host_target" ]]; then
  echo "could not determine host target triple" >&2
  exit 1
fi

mkdir -p "$output_dir"
rm -f "$output_dir"/host_coverage_summary.tsv "$output_dir"/*.lcov "$merged_lcov_path"

# scripts/host-suites.tsv is the sole test/lint/coverage membership registry;
# read the coverage=yes manifests (and their invocation mode) from it rather
# than hardcoding a list here.
registry="$repo_root/scripts/host-suites.tsv"
declare -a manifests=()
declare -a modes=()
while IFS=$'\t' read -r manifest mode; do
  manifests+=("$manifest")
  modes+=("$mode")
done < <(awk -F'\t' 'NR>1 && $4=="yes" { print $6"\t"$5 }' "$registry")

fail=0
for i in "${!manifests[@]}"; do
  manifest_rel="${manifests[$i]}"
  mode="${modes[$i]}"
  crate_name="$(basename "$(dirname "$manifest_rel")")"
  manifest_path="$repo_root/$manifest_rel"
  lcov_path="$output_dir/${crate_name}.lcov"

  declare -a extra_env=()
  if [[ "$mode" == "crate-lvgl" ]]; then
    extra_env=(DEP_LV_CONFIG_PATH="$repo_root/config/lvgl")
  fi

  echo "coverage: $crate_name"
  (
    cd /tmp
    env "${extra_env[@]}" \
      RUSTUP_TOOLCHAIN="$toolchain" cargo llvm-cov \
      --locked \
      --manifest-path "$manifest_path" \
      --target "$host_target" \
      --lcov \
      --output-path "$lcov_path" \
      "$@" >/dev/null
  )

  line_cov="$(
    awk -F: '
      /^LF:/ { lf += $2 }
      /^LH:/ { lh += $2 }
      END {
        if (lf == 0) {
          print "0.00"
        } else {
          printf "%.2f", (lh * 100.0 / lf)
        }
      }
    ' "$lcov_path"
  )"
  printf '%s\t%s\n' "$crate_name" "$line_cov" >>"$output_dir/host_coverage_summary.tsv"

  if awk "BEGIN { exit !($line_cov < $min_line_coverage) }"; then
    echo "coverage gate failed for $crate_name: ${line_cov}% < ${min_line_coverage}%"
    fail=1
  fi
done

cat "$output_dir"/*.lcov >"$merged_lcov_path"

echo
echo "host line coverage summary"
awk 'BEGIN { printf "%-28s %s\n", "crate", "line_coverage(%)" } { printf "%-28s %s\n", $1, $2 }' \
  "$output_dir/host_coverage_summary.tsv"
echo "artifacts: $output_dir"
echo "merged_lcov: $merged_lcov_path"

exit "$fail"
